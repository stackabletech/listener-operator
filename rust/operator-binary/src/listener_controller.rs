use crate::utils::node_primary_address;
use futures::{future::try_join_all, StreamExt};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    builder::OwnerReferenceBuilder,
    commons::listener::{
        Listener, ListenerClass, ListenerIngress, ListenerPort, ListenerSpec, ListenerStatus,
        ServiceType,
    },
    k8s_openapi::api::core::v1::{Endpoints, Node, Service, ServicePort, ServiceSpec},
    kube::{
        api::{DynamicObject, ListParams, ObjectMeta},
        runtime::{controller, reflector::ObjectRef},
    },
    logging::controller::{report_controller_reconciled, ReconcilerError},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use strum::IntoStaticStr;

const FIELD_MANAGER_SCOPE: &str = "listener";

pub async fn run(client: stackable_operator::client::Client) {
    let controller =
        controller::Controller::new(client.get_all_api::<Listener>(), ListParams::default());
    let listener_store = controller.store();
    controller
        .owns(client.get_all_api::<Service>(), ListParams::default())
        .watches(
            client.get_all_api::<Endpoints>(),
            ListParams::default(),
            move |endpoints| {
                listener_store
                    .state()
                    .into_iter()
                    .filter(move |listener| {
                        listener
                            .status
                            .as_ref()
                            .and_then(|s| s.service_name.as_deref())
                            == endpoints.metadata.name.as_deref()
                    })
                    .map(|l| ObjectRef::from_obj(&*l))
            },
        )
        .shutdown_on_signal()
        .run(
            reconcile,
            error_policy,
            Arc::new(Ctx {
                client: client.clone(),
            }),
        )
        .map(|res| {
            report_controller_reconciled(&client, "listener.listeners.stackable.tech", &res);
        })
        .collect::<()>()
        .await;
}

pub struct Ctx {
    pub client: stackable_operator::client::Client,
}

#[derive(Debug, Snafu, IntoStaticStr)]
pub enum Error {
    #[snafu(display("object has no namespace"))]
    NoNs,
    #[snafu(display("object has no name"))]
    NoName,
    #[snafu(display("object has no ListenerClass (.spec.class_name)"))]
    NoListenerClass,
    #[snafu(display("failed to get {obj}"))]
    GetObject {
        source: stackable_operator::error::Error,
        obj: ObjectRef<DynamicObject>,
    },
    #[snafu(display("failed to build owner reference to Listener"))]
    BuildListenerOwnerRef {
        source: stackable_operator::error::Error,
    },
    #[snafu(display("failed to apply {svc}"))]
    ApplyService {
        source: stackable_operator::error::Error,
        svc: ObjectRef<Service>,
    },
    #[snafu(display("failed to apply status for Listener"))]
    ApplyStatus {
        source: stackable_operator::error::Error,
    },
}
impl ReconcilerError for Error {
    fn category(&self) -> &'static str {
        self.into()
    }

    fn secondary_object(&self) -> Option<ObjectRef<DynamicObject>> {
        match self {
            Self::NoNs => None,
            Self::NoName => None,
            Self::NoListenerClass => None,
            Self::GetObject { source: _, obj } => Some(obj.clone()),
            Self::BuildListenerOwnerRef { .. } => None,
            Self::ApplyService { source: _, svc } => Some(svc.clone().erase()),
            Self::ApplyStatus { source: _ } => None,
        }
    }
}

pub async fn reconcile(
    listener: Arc<Listener>,
    ctx: Arc<Ctx>,
) -> Result<controller::Action, Error> {
    let ns = listener.metadata.namespace.clone().context(NoNsSnafu)?;
    let listener_class_name = listener
        .spec
        .class_name
        .as_deref()
        .context(NoListenerClassSnafu)?;
    let listener_class = ctx
        .client
        .get::<ListenerClass>(listener_class_name, None)
        .await
        .with_context(|_| GetObjectSnafu {
            obj: ObjectRef::<ListenerClass>::new(listener_class_name).erase(),
        })?;
    let pod_ports = listener
        .spec
        .ports
        .iter()
        .flatten()
        .map(
            |ListenerPort {
                 name,
                 port,
                 protocol,
             }| {
                (
                    (protocol, name),
                    ServicePort {
                        name: Some(name.clone()),
                        protocol: protocol.clone(),
                        port: *port,
                        ..Default::default()
                    },
                )
            },
        )
        // Deduplicate ports by (protocol, name)
        .collect::<BTreeMap<_, ServicePort>>();
    let svc_name = listener.metadata.name.clone().context(NoNameSnafu)?;
    let svc = Service {
        metadata: ObjectMeta {
            namespace: Some(ns.clone()),
            name: Some(svc_name.clone()),
            owner_references: Some(vec![OwnerReferenceBuilder::new()
                .initialize_from_resource(&*listener)
                .build()
                .context(BuildListenerOwnerRefSnafu)?]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some(match listener_class.spec.service_type {
                ServiceType::NodePort => "NodePort".to_string(),
                ServiceType::LoadBalancer => "LoadBalancer".to_string(),
            }),
            ports: Some(pod_ports.into_values().collect()),
            external_traffic_policy: Some("Local".to_string()),
            selector: listener.spec.pod_selector.clone(),
            publish_not_ready_addresses: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let svc = ctx
        .client
        .apply_patch(FIELD_MANAGER_SCOPE, &svc, &svc)
        .await
        .with_context(|_| ApplyServiceSnafu {
            svc: ObjectRef::from_obj(&svc),
        })?;

    let addresses: Vec<String>;
    let ports: BTreeMap<String, i32>;
    match listener_class.spec.service_type {
        ServiceType::NodePort => {
            let endpoints = ctx
                .client
                .get_opt::<Endpoints>(&svc_name, Some(&ns))
                .await
                .with_context(|_| GetObjectSnafu {
                    obj: ObjectRef::<Endpoints>::new(&svc_name).within(&ns).erase(),
                })?
                // Endpoints object may not yet be created by its respective controller
                .unwrap_or_default();
            let node_names = endpoints
                .subsets
                .into_iter()
                .flatten()
                .flat_map(|subset| subset.addresses)
                .flatten()
                .flat_map(|addr| addr.node_name)
                .collect::<Vec<_>>();
            let nodes = try_join_all(node_names.iter().map(|node_name| async {
                ctx.client
                    .get::<Node>(node_name, None)
                    .await
                    .context(GetObjectSnafu {
                        obj: ObjectRef::<Node>::new(node_name).erase(),
                    })
            }))
            .await?;
            addresses = nodes
                .into_iter()
                .flat_map(|node| node_primary_address(&node).map(str::to_string))
                .collect();
            ports = svc
                .spec
                .as_ref()
                .and_then(|s| s.ports.as_ref())
                .into_iter()
                .flatten()
                .filter_map(|port| Some((port.name.clone()?, port.node_port?)))
                .collect();
        }
        ServiceType::LoadBalancer => {
            addresses = svc
                .status
                .iter()
                .flat_map(|ss| ss.load_balancer.as_ref()?.ingress.as_ref())
                .flatten()
                .flat_map(|ingress| ingress.hostname.clone().or_else(|| ingress.ip.clone()))
                .collect();
            ports = svc
                .spec
                .as_ref()
                .and_then(|s| s.ports.as_ref())
                .into_iter()
                .flatten()
                .filter_map(|port| Some((port.name.clone()?, port.port)))
                .collect();
        }
    };

    let listener_status_meta = Listener {
        metadata: ObjectMeta {
            name: listener.metadata.name.clone(),
            namespace: listener.metadata.namespace.clone(),
            uid: listener.metadata.uid.clone(),
            ..Default::default()
        },
        spec: ListenerSpec::default(),
        status: None,
    };
    let listener_status = ListenerStatus {
        service_name: svc.metadata.name,
        ingress_addresses: Some(
            addresses
                .into_iter()
                .map(|addr| ListenerIngress {
                    address: addr,
                    ports: ports.clone(),
                })
                .collect(),
        ),
        node_ports: (listener_class.spec.service_type == ServiceType::NodePort).then(|| ports),
    };
    ctx.client
        .apply_patch_status(FIELD_MANAGER_SCOPE, &listener_status_meta, &listener_status)
        .await
        .context(ApplyStatusSnafu)?;

    Ok(controller::Action::await_change())
}

pub fn error_policy(_err: &Error, _ctx: Arc<Ctx>) -> controller::Action {
    controller::Action::requeue(Duration::from_secs(5))
}
