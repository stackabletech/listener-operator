use crate::{
    crd::{
        LoadBalancer, LoadBalancerClass, LoadBalancerIngress, LoadBalancerPort, LoadBalancerSpec,
        LoadBalancerStatus, ServiceType,
    },
    utils::node_primary_address,
};
use futures::{future::try_join_all, StreamExt};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    builder::OwnerReferenceBuilder,
    k8s_openapi::api::core::v1::{Endpoints, Node, Service, ServicePort, ServiceSpec},
    kube::{
        api::{DynamicObject, ListParams, ObjectMeta},
        runtime::{controller, reflector::ObjectRef},
    },
    logging::controller::{report_controller_reconciled, ReconcilerError},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use strum::IntoStaticStr;

const FIELD_MANAGER_SCOPE: &str = "loadbalancer";

pub async fn run(client: stackable_operator::client::Client) {
    let controller =
        controller::Controller::new(client.get_all_api::<LoadBalancer>(), ListParams::default());
    let lb_store = controller.store();
    controller
        .owns(client.get_all_api::<Service>(), ListParams::default())
        .watches(
            client.get_all_api::<Endpoints>(),
            ListParams::default(),
            move |endpoints| {
                lb_store
                    .state()
                    .into_iter()
                    .filter(move |lb| {
                        lb.status
                            .as_ref()
                            .and_then(|lbs| lbs.service_name.as_deref())
                            == endpoints.metadata.name.as_deref()
                    })
                    .map(|lb| ObjectRef::from_obj(&*lb))
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
            report_controller_reconciled(&client, "loadbalancers.lb.stackable.tech", &res);
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
    #[snafu(display("object has no LoadBalancerClass (.spec.class_name)"))]
    NoLbClass,
    #[snafu(display("failed to get {obj}"))]
    GetObject {
        source: stackable_operator::error::Error,
        obj: ObjectRef<DynamicObject>,
    },
    #[snafu(display("failed to build owner reference to LoadBalancer"))]
    BuildLbOwnerRef {
        source: stackable_operator::error::Error,
    },
    #[snafu(display("failed to apply {svc}"))]
    ApplyService {
        source: stackable_operator::error::Error,
        svc: ObjectRef<Service>,
    },
    #[snafu(display("failed to apply status for LoadBalancer"))]
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
            Self::NoLbClass => None,
            Self::GetObject { source: _, obj } => Some(obj.clone()),
            Self::BuildLbOwnerRef { .. } => None,
            Self::ApplyService { source: _, svc } => Some(svc.clone().erase()),
            Self::ApplyStatus { source: _ } => None,
        }
    }
}

pub async fn reconcile(lb: Arc<LoadBalancer>, ctx: Arc<Ctx>) -> Result<controller::Action, Error> {
    let ns = lb.metadata.namespace.clone().context(NoNsSnafu)?;
    let lb_class_name = lb.spec.class_name.as_deref().context(NoLbClassSnafu)?;
    let lb_class = ctx
        .client
        .get::<LoadBalancerClass>(lb_class_name, None)
        .await
        .with_context(|_| GetObjectSnafu {
            obj: ObjectRef::<LoadBalancerClass>::new(lb_class_name).erase(),
        })?;
    let pod_ports = lb
        .spec
        .ports
        .iter()
        .flatten()
        .map(
            |LoadBalancerPort {
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
    let svc_name = lb.metadata.name.clone().context(NoNameSnafu)?;
    let svc = Service {
        metadata: ObjectMeta {
            namespace: Some(ns.clone()),
            name: Some(svc_name.clone()),
            owner_references: Some(vec![OwnerReferenceBuilder::new()
                .initialize_from_resource(&*lb)
                .build()
                .context(BuildLbOwnerRefSnafu)?]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some(match lb_class.spec.service_type {
                ServiceType::NodePort => "NodePort".to_string(),
                ServiceType::LoadBalancer => "LoadBalancer".to_string(),
            }),
            ports: Some(pod_ports.into_values().collect()),
            external_traffic_policy: Some("Local".to_string()),
            selector: lb.spec.pod_selector.clone(),
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
    match lb_class.spec.service_type {
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

    let lb_status_meta = LoadBalancer {
        metadata: ObjectMeta {
            name: lb.metadata.name.clone(),
            namespace: lb.metadata.namespace.clone(),
            uid: lb.metadata.uid.clone(),
            ..Default::default()
        },
        spec: LoadBalancerSpec::default(),
        status: None,
    };
    let lb_status = LoadBalancerStatus {
        service_name: svc.metadata.name,
        ingress_addresses: Some(
            addresses
                .into_iter()
                .map(|addr| LoadBalancerIngress {
                    address: addr,
                    ports: ports.clone(),
                })
                .collect(),
        ),
        node_ports: (lb_class.spec.service_type == ServiceType::NodePort).then(|| ports),
    };
    ctx.client
        .apply_patch_status(FIELD_MANAGER_SCOPE, &lb_status_meta, &lb_status)
        .await
        .context(ApplyStatusSnafu)?;

    Ok(controller::Action::await_change())
}

pub fn error_policy(_err: &Error, _ctx: Arc<Ctx>) -> controller::Action {
    controller::Action::requeue(Duration::from_secs(5))
}
