use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use futures::{
    future::{try_join, try_join_all},
    StreamExt,
};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    builder::meta::OwnerReferenceBuilder,
    commons::listener::{
        AddressType, Listener, ListenerClass, ListenerIngress, ListenerPort, ListenerSpec,
        ListenerStatus, ServiceType,
    },
    k8s_openapi::{
        api::core::v1::{Endpoints, Node, PersistentVolume, Service, ServicePort, ServiceSpec},
        apimachinery::pkg::apis::meta::v1::LabelSelector,
    },
    kube::{
        api::{DynamicObject, ObjectMeta},
        runtime::{controller, reflector::ObjectRef, watcher},
        ResourceExt,
    },
    logging::controller::{report_controller_reconciled, ReconcilerError},
    time::Duration,
};
use strum::IntoStaticStr;

use crate::{
    csi_server::node::NODE_TOPOLOGY_LABEL_HOSTNAME,
    utils::{node_primary_address, AddressCandidates},
};

#[cfg(doc)]
use stackable_operator::k8s_openapi::api::core::v1::Pod;

const FIELD_MANAGER_SCOPE: &str = "listener";

pub async fn run(client: stackable_operator::client::Client) {
    let controller =
        controller::Controller::new(client.get_all_api::<Listener>(), watcher::Config::default());
    let listener_store = controller.store();
    controller
        .owns(client.get_all_api::<Service>(), watcher::Config::default())
        .watches(
            client.get_all_api::<ListenerClass>(),
            watcher::Config::default(),
            {
                let listener_store = listener_store.clone();
                move |listenerclass| {
                    listener_store
                        .state()
                        .into_iter()
                        .filter(move |listener| {
                            listener.spec.class_name == listenerclass.metadata.name
                        })
                        .map(|l| ObjectRef::from_obj(&*l))
                }
            },
        )
        .watches(
            client.get_all_api::<Endpoints>(),
            watcher::Config::default(),
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
        .watches(
            client.get_all_api::<PersistentVolume>(),
            watcher::Config::default(),
            |pv| {
                let labels = pv.labels();
                labels
                    .get(PV_LABEL_LISTENER_NAMESPACE)
                    .zip(labels.get(PV_LABEL_LISTENER_NAME))
                    .map(|(ns, name)| ObjectRef::<Listener>::new(name).within(ns))
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

    #[snafu(display("failed to generate Listener's PersistentVolume selector"))]
    ListenerPvSelector {
        source: ListenerPersistentVolumeLabelError,
    },

    #[snafu(display("failed to generate Listener's Pod selector"))]
    ListenerPodSelector {
        source: ListenerMountedPodLabelError,
    },

    #[snafu(display("failed to get PersistentVolumes for Listener"))]
    GetListenerPvs {
        source: stackable_operator::client::Error,
    },

    #[snafu(display("failed to get {obj}"))]
    GetObject {
        source: stackable_operator::client::Error,
        obj: ObjectRef<DynamicObject>,
    },

    #[snafu(display("failed to build owner reference to Listener"))]
    BuildListenerOwnerRef {
        source: stackable_operator::builder::meta::Error,
    },

    #[snafu(display("failed to apply {svc}"))]
    ApplyService {
        source: stackable_operator::client::Error,
        svc: ObjectRef<Service>,
    },

    #[snafu(display("failed to apply status for Listener"))]
    ApplyStatus {
        source: stackable_operator::client::Error,
    },
}
type Result<T, E = Error> = std::result::Result<T, E>;
impl ReconcilerError for Error {
    fn category(&self) -> &'static str {
        self.into()
    }

    fn secondary_object(&self) -> Option<ObjectRef<DynamicObject>> {
        match self {
            Self::NoNs => None,
            Self::NoName => None,
            Self::NoListenerClass => None,
            Self::ListenerPvSelector { source: _ } => None,
            Self::ListenerPodSelector { source: _ } => None,
            Self::GetListenerPvs { source: _ } => None,
            Self::GetObject { source: _, obj } => Some(obj.clone()),
            Self::BuildListenerOwnerRef { .. } => None,
            Self::ApplyService { source: _, svc } => Some(svc.clone().erase()),
            Self::ApplyStatus { source: _ } => None,
        }
    }
}

pub async fn reconcile(listener: Arc<Listener>, ctx: Arc<Ctx>) -> Result<controller::Action> {
    let ns = listener.metadata.namespace.as_deref().context(NoNsSnafu)?;
    let listener_class_name = listener
        .spec
        .class_name
        .as_deref()
        .context(NoListenerClassSnafu)?;
    let listener_class = ctx
        .client
        .get::<ListenerClass>(listener_class_name, &())
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
    let mut pod_selector = listener.spec.extra_pod_selector_labels.clone();
    pod_selector.extend([listener_mounted_pod_label(&listener).context(ListenerPodSelectorSnafu)?]);

    // ClusterIP services have no external traffic to apply policies to
    let external_traffic_policy = match listener_class.spec.service_type {
        ServiceType::NodePort | ServiceType::LoadBalancer => Some(
            listener_class
                .spec
                .service_external_traffic_policy
                .to_string(),
        ),
        ServiceType::ClusterIP => None,
    };

    let svc = Service {
        metadata: ObjectMeta {
            namespace: Some(ns.to_string()),
            name: Some(svc_name.clone()),
            owner_references: Some(vec![OwnerReferenceBuilder::new()
                .initialize_from_resource(&*listener)
                .build()
                .context(BuildListenerOwnerRefSnafu)?]),
            // Propagate the labels from the Listener object to the Service object, so it can be found easier
            labels: listener.metadata.labels.clone(),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            // We explicitly match here and do not implement `ToString` as there might be more (non vanilla k8s Service
            // types) in the future.
            type_: Some(match listener_class.spec.service_type {
                ServiceType::NodePort => "NodePort".to_string(),
                ServiceType::LoadBalancer => "LoadBalancer".to_string(),
                ServiceType::ClusterIP => "ClusterIP".to_string(),
            }),
            ports: Some(pod_ports.into_values().collect()),
            external_traffic_policy,
            selector: Some(pod_selector),
            publish_not_ready_addresses: Some(
                listener
                    .spec
                    .publish_not_ready_addresses
                    .unwrap_or_default(),
            ),
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

    let nodes: Vec<Node>;
    let kubernetes_service_fqdn: String;
    let addresses: Vec<(&str, AddressType)>;
    let ports: BTreeMap<String, i32>;
    match listener_class.spec.service_type {
        ServiceType::NodePort => {
            let node_names =
                node_names_for_nodeport_listener(&ctx.client, &listener, ns, &svc_name).await?;
            nodes = try_join_all(node_names.iter().map(|node_name| async {
                ctx.client
                    .get::<Node>(node_name, &())
                    .await
                    .context(GetObjectSnafu {
                        obj: ObjectRef::<Node>::new(node_name).erase(),
                    })
            }))
            .await?;
            addresses = nodes
                .iter()
                .flat_map(|node| {
                    node_primary_address(node).pick(listener_class.spec.preferred_address_type)
                })
                .collect::<Vec<_>>();
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
                .flat_map(|ingress| {
                    AddressCandidates {
                        ip: ingress.ip.as_deref(),
                        hostname: ingress.hostname.as_deref(),
                    }
                    .pick(listener_class.spec.preferred_address_type)
                })
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
        ServiceType::ClusterIP => {
            addresses = match listener_class.spec.preferred_address_type {
                AddressType::Ip => svc
                    .spec
                    .iter()
                    .flat_map(|s| &s.cluster_ips)
                    .flatten()
                    .map(|addr| (&**addr, AddressType::Ip))
                    .collect::<Vec<_>>(),
                AddressType::Hostname => {
                    kubernetes_service_fqdn = format!("{svc_name}.{ns}.svc.cluster.local");
                    vec![(&kubernetes_service_fqdn, AddressType::Hostname)]
                }
            };
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
                .map(|(address, address_type)| ListenerIngress {
                    address: address.to_string(),
                    address_type,
                    ports: ports.clone(),
                })
                .collect(),
        ),
        node_ports: (listener_class.spec.service_type == ServiceType::NodePort).then_some(ports),
    };
    ctx.client
        .apply_patch_status(FIELD_MANAGER_SCOPE, &listener_status_meta, &listener_status)
        .await
        .context(ApplyStatusSnafu)?;

    Ok(controller::Action::await_change())
}

pub fn error_policy<T>(_obj: Arc<T>, _error: &Error, _ctx: Arc<Ctx>) -> controller::Action {
    controller::Action::requeue(*Duration::from_secs(5))
}

/// Lists the names of the [`Node`]s backing this [`Listener`].
///
/// Should only be used for [`NodePort`](`ServiceType::NodePort`) [`Listener`]s.
async fn node_names_for_nodeport_listener(
    client: &stackable_operator::client::Client,
    listener: &Listener,
    namespace: &str,
    service_name: &str,
) -> Result<BTreeSet<String>> {
    let (pvs, endpoints) = try_join(
        async {
            client
                .list_with_label_selector::<PersistentVolume>(
                    &(),
                    &LabelSelector {
                        match_labels: Some(listener_persistent_volume_label(listener).unwrap()),
                        ..Default::default()
                    },
                )
                .await
                .context(GetListenerPvsSnafu)
        },
        async {
            client
                // Endpoints object may not yet be created by its respective controller
                .get_opt::<Endpoints>(service_name, namespace)
                .await
                .with_context(|_| GetObjectSnafu {
                    obj: ObjectRef::<Endpoints>::new(service_name)
                        .within(namespace)
                        .erase(),
                })
        },
    )
    .await?;

    let pv_node_names = pvs
        .into_iter()
        .filter_map(|pv| pv.spec?.node_affinity?.required)
        .flat_map(|affinity| affinity.node_selector_terms)
        .filter_map(|terms| terms.match_expressions)
        .flatten()
        .filter(|expr| expr.key == NODE_TOPOLOGY_LABEL_HOSTNAME && expr.operator == "In")
        .filter_map(|expr| expr.values)
        .flatten()
        .collect::<BTreeSet<_>>();

    // Old objects that haven't been mounted before the PV lookup mechanism was added will
    // not have the correct labels, so we also look up using Endpoints.
    let endpoints_node_names = endpoints
        .into_iter()
        .filter_map(|endpoints| endpoints.subsets)
        .flatten()
        .flat_map(|subset| subset.addresses)
        .flatten()
        .flat_map(|addr| addr.node_name)
        .collect::<BTreeSet<_>>();

    let node_names_missing_from_pv = endpoints_node_names
        .difference(&pv_node_names)
        .collect::<Vec<_>>();
    if !node_names_missing_from_pv.is_empty() {
        tracing::warn!(
            ?node_names_missing_from_pv,
            "some backing Nodes could only be found via legacy Endpoints discovery method, \
            this may cause discovery config to be unstable \
            (hint: try restarting the Pods backing this Listener)",
        );
    }

    let mut node_names = pv_node_names;
    node_names.extend(endpoints_node_names);
    Ok(node_names)
}

#[derive(Snafu, Debug)]
#[snafu(module)]
pub enum ListenerMountedPodLabelError {
    #[snafu(display("object has no uid"))]
    NoUid,
    #[snafu(display("object has no name"))]
    NoName,
}

/// A label that identifies [`Pod`]s that have mounted `listener`
///
/// Listener-Op's CSI Node driver is responsible for adding this to the relevant [`Pod`]s.
pub fn listener_mounted_pod_label(
    listener: &Listener,
) -> Result<(String, String), ListenerMountedPodLabelError> {
    use listener_mounted_pod_label_error::*;
    let uid = listener.metadata.uid.as_deref().context(NoUidSnafu)?;
    // Labels names are limited to 63 characters, prefix "listener.stackable.tech/mnt." takes 28 characters,
    // A UUID is 36 characters (for a total of 64), but by stripping out the meaningless dashes we can squeeze into
    // 60.
    // We prefer uid over name because uids have a consistent length.
    Ok((
        // This should probably have been listeners.stackable.tech/ instead, but too late to change now
        format!("listener.stackable.tech/mnt.{}", uid.replace('-', "")),
        // Arbitrary, but (hopefully) helps indicate to users which listener it applies to
        listener.metadata.name.clone().context(NoNameSnafu)?,
    ))
}

#[derive(Snafu, Debug)]
#[snafu(module)]
pub enum ListenerPersistentVolumeLabelError {
    #[snafu(display("object has no name"))]
    NoName,

    #[snafu(display("object has no namespace"))]
    NoNamespace,
}

const PV_LABEL_LISTENER_NAMESPACE: &str = "listeners.stackable.tech/listener-namespace";
const PV_LABEL_LISTENER_NAME: &str = "listeners.stackable.tech/listener-name";

/// A label that identifies which [`Listener`] corresponds to a given [`PersistentVolume`].
pub fn listener_persistent_volume_label(
    listener: &Listener,
) -> Result<BTreeMap<String, String>, ListenerPersistentVolumeLabelError> {
    use listener_persistent_volume_label_error::*;
    Ok([
        (
            PV_LABEL_LISTENER_NAMESPACE.to_string(),
            listener
                .metadata
                .namespace
                .clone()
                .context(NoNamespaceSnafu)?,
        ),
        (
            PV_LABEL_LISTENER_NAME.to_string(),
            listener.metadata.name.clone().context(NoNameSnafu)?,
        ),
    ]
    .into())
}
