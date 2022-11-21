use csi_grpc::{self as csi, v1::Topology};
use serde::{de::IntoDeserializer, Deserialize};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    builder::OwnerReferenceBuilder,
    commons::listener::{Listener, ListenerIngress, ListenerPort, ListenerSpec},
    k8s_openapi::api::core::v1::{Node, PersistentVolume, Pod},
    kube::{
        core::{DynamicObject, ObjectMeta},
        runtime::reflector::ObjectRef,
    },
};
use std::{fmt::Debug, path::PathBuf};
use tonic::{Request, Response, Status};

use crate::{
    listener_controller::listener_mounted_pod_label,
    utils::{error_full_message, node_primary_address},
};

use super::{tonic_unimplemented, ListenerSelector, ListenerVolumeContext};

const FIELD_MANAGER_SCOPE: &str = "volume";

pub struct ListenerOperatorNode {
    pub client: stackable_operator::client::Client,
    pub node_name: String,
}

#[derive(Deserialize)]
struct ListenerNodeVolumeContext {
    #[serde(rename = "csi.storage.k8s.io/pod.namespace")]
    pod_namespace: String,
    #[serde(rename = "csi.storage.k8s.io/pod.name")]
    pod_name: String,
    #[serde(flatten)]
    common: ListenerVolumeContext,
}

#[derive(Snafu, Debug)]
#[snafu(module)]
enum PublishVolumeError {
    #[snafu(display("failed to decode volume context"))]
    DecodeVolumeContext { source: serde::de::value::Error },
    #[snafu(display("failed to get {obj}"))]
    GetObject {
        source: stackable_operator::error::Error,
        obj: ObjectRef<DynamicObject>,
    },
    #[snafu(display("{pod} has not been scheduled to a node yet"))]
    PodHasNoNode { pod: ObjectRef<Pod> },
    #[snafu(display("failed to build Listener's owner reference"))]
    BuildListenerOwnerRef {
        source: stackable_operator::error::Error,
    },
    #[snafu(display("failed to apply {listener}"))]
    ApplyListener {
        source: stackable_operator::error::Error,
        listener: ObjectRef<Listener>,
    },
    #[snafu(display("failed to add listener label to {pod}"))]
    AddListenerLabelToPod {
        source: stackable_operator::error::Error,
        pod: ObjectRef<Pod>,
    },
    #[snafu(display("failed to prepare pod dir at {target_path:?}"))]
    PreparePodDir {
        source: pod_dir::Error,
        target_path: PathBuf,
    },
}

impl From<PublishVolumeError> for Status {
    fn from(err: PublishVolumeError) -> Self {
        let full_msg = error_full_message(&err);
        // Convert to an appropriate tonic::Status representation and include full error message
        match err {
            PublishVolumeError::DecodeVolumeContext { .. } => Status::invalid_argument(full_msg),
            PublishVolumeError::GetObject { .. } => Status::unavailable(full_msg),
            PublishVolumeError::PodHasNoNode { .. } => Status::unavailable(full_msg),
            PublishVolumeError::BuildListenerOwnerRef { .. } => Status::unavailable(full_msg),
            PublishVolumeError::ApplyListener { .. } => Status::unavailable(full_msg),
            PublishVolumeError::AddListenerLabelToPod { .. } => Status::unavailable(full_msg),
            PublishVolumeError::PreparePodDir { .. } => Status::internal(full_msg),
        }
    }
}

#[derive(Snafu, Debug)]
#[snafu(module)]
enum UnpublishVolumeError {
    #[snafu(display("failed to clean up volume data at {path:?}"))]
    CleanupData {
        source: std::io::Error,
        path: PathBuf,
    },
}

impl From<UnpublishVolumeError> for Status {
    fn from(err: UnpublishVolumeError) -> Self {
        let full_msg = error_full_message(&err);
        // Convert to an appropriate tonic::Status representation and include full error message
        match err {
            UnpublishVolumeError::CleanupData { .. } => Status::internal(full_msg),
        }
    }
}

#[tonic::async_trait]
impl csi::v1::node_server::Node for ListenerOperatorNode {
    async fn node_get_info(
        &self,
        _request: Request<csi::v1::NodeGetInfoRequest>,
    ) -> Result<Response<csi::v1::NodeGetInfoResponse>, Status> {
        Ok(Response::new(csi::v1::NodeGetInfoResponse {
            node_id: self.node_name.clone(),
            max_volumes_per_node: i64::MAX,
            accessible_topology: Some(Topology {
                segments: [(
                    "listeners.stackable.tech/hostname".to_string(),
                    self.node_name.clone(),
                )]
                .into(),
            }),
        }))
    }

    async fn node_get_capabilities(
        &self,
        _request: Request<csi::v1::NodeGetCapabilitiesRequest>,
    ) -> Result<Response<csi::v1::NodeGetCapabilitiesResponse>, Status> {
        Ok(Response::new(csi::v1::NodeGetCapabilitiesResponse {
            capabilities: Vec::new(),
        }))
    }

    async fn node_publish_volume(
        &self,
        request: Request<csi::v1::NodePublishVolumeRequest>,
    ) -> Result<Response<csi::v1::NodePublishVolumeResponse>, Status> {
        use publish_volume_error::*;

        let request = request.into_inner();
        let ListenerNodeVolumeContext {
            pod_namespace: ns,
            pod_name,
            common: ListenerVolumeContext { listener_selector },
        } = ListenerNodeVolumeContext::deserialize(request.volume_context.into_deserializer())
            .context(DecodeVolumeContextSnafu)?;
        let pv_name = &request.volume_id;

        let pv = self
            .client
            .get::<PersistentVolume>(pv_name, &())
            .await
            .with_context(|_| GetObjectSnafu {
                obj: {
                    let obj = ObjectRef::<PersistentVolume>::new(pv_name);
                    obj.erase()
                },
            })?;

        let pod = self
            .client
            .get::<Pod>(&pod_name, &ns)
            .await
            .with_context(|_| GetObjectSnafu {
                obj: { ObjectRef::<Pod>::new(&pod_name).erase() },
            })?;

        let listener = match listener_selector {
            ListenerSelector::Listener(listener_name) => self
                .client
                .get::<Listener>(&listener_name, &ns)
                .await
                .with_context(|_| GetObjectSnafu {
                    obj: {
                        ObjectRef::<Listener>::new(&listener_name)
                            .within(&ns)
                            .erase()
                    },
                })?,
            ListenerSelector::ListenerClass(listener_class_name) => {
                let listener = Listener {
                    metadata: ObjectMeta {
                        namespace: Some(ns.clone()),
                        name: pv
                            .spec
                            .as_ref()
                            .and_then(|pv_spec| pv_spec.claim_ref.as_ref()?.name.clone()),
                        owner_references: Some(vec![OwnerReferenceBuilder::new()
                            .initialize_from_resource(&pv)
                            .build()
                            .context(BuildListenerOwnerRefSnafu)?]),
                        ..Default::default()
                    },
                    spec: ListenerSpec {
                        class_name: Some(listener_class_name),
                        ports: Some(
                            pod.spec
                                .iter()
                                .flat_map(|ps| &ps.containers)
                                .flat_map(|ctr| &ctr.ports)
                                .flatten()
                                .map(|port| ListenerPort {
                                    name: port
                                        .name
                                        .clone()
                                        .unwrap_or_else(|| format!("port-{}", port.container_port)),
                                    protocol: port.protocol.clone(),
                                    port: port.container_port,
                                })
                                .collect(),
                        ),
                        publish_not_ready_addresses: Some(true),
                        ..Default::default()
                    },
                    status: None,
                };
                self.client
                    .apply_patch(FIELD_MANAGER_SCOPE, &listener, &listener)
                    .await
                    .with_context(|_| ApplyListenerSnafu {
                        listener: ObjectRef::from_obj(&listener),
                    })?
            }
        };

        // Add listener label to pod so that traffic can be directed to it
        self.client
            // IMPORTANT
            // Use a merge patch rather than an apply so that we don't delete labels added by other listener volumes.
            // Volumes aren't hot-swappable anyway, and all labels will be removed when the pod is deleted.
            .merge_patch(
                &pod,
                &Pod {
                    metadata: ObjectMeta {
                        labels: Some([listener_mounted_pod_label(&listener)].into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
            .with_context(|_| AddListenerLabelToPodSnafu {
                pod: ObjectRef::from_obj(&pod),
            })?;

        // Prefer calculating a per-node address where possible, to ensure that the address at least tries to
        // connect to the pod in question.
        // We also can't rely on `ingress_addresses` being set yet, since the pod won't have an IP address yet
        // (and so can't be found in `Endpoints`)
        let listener_addrs = if let Some(node_ports) = listener
            .status
            .as_ref()
            .and_then(|status| status.node_ports.clone())
        {
            let node_name = pod
                .spec
                .as_ref()
                .and_then(|s| s.node_name.as_deref())
                .with_context(|| PodHasNoNodeSnafu {
                    pod: ObjectRef::from_obj(&pod),
                })?;
            let node = self
                .client
                .get::<Node>(&node_name, &())
                .await
                .with_context(|_| GetObjectSnafu {
                    obj: { ObjectRef::<Node>::new(&node_name).erase() },
                })?;

            node_primary_address(&node)
                .map(|address| ListenerIngress {
                    address: address.to_string(),
                    ports: node_ports,
                })
                .into_iter()
                .collect()
        } else {
            listener
                .status
                .as_ref()
                .and_then(|s| s.ingress_addresses.as_ref())
                .cloned()
                .unwrap_or_default()
        };

        let target_path = PathBuf::from(request.target_path);
        pod_dir::write_listener_info_to_pod_dir(&target_path, &listener_addrs)
            .await
            .context(PreparePodDirSnafu { target_path })?;

        Ok(Response::new(csi::v1::NodePublishVolumeResponse {}))
    }

    async fn node_unpublish_volume(
        &self,
        request: Request<csi::v1::NodeUnpublishVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeUnpublishVolumeResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.target_path);
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // already deleted => nothing to do
            }
            Err(err) => Err(err).context(unpublish_volume_error::CleanupDataSnafu { path })?,
        }
        Ok(Response::new(csi::v1::NodeUnpublishVolumeResponse {}))
    }

    async fn node_stage_volume(
        &self,
        _request: Request<csi::v1::NodeStageVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeStageVolumeResponse>, Status> {
        tonic_unimplemented()
    }

    async fn node_unstage_volume(
        &self,
        _request: Request<csi::v1::NodeUnstageVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeUnstageVolumeResponse>, Status> {
        tonic_unimplemented()
    }

    async fn node_get_volume_stats(
        &self,
        _request: Request<csi::v1::NodeGetVolumeStatsRequest>,
    ) -> Result<Response<csi::v1::NodeGetVolumeStatsResponse>, Status> {
        tonic_unimplemented()
    }

    async fn node_expand_volume(
        &self,
        _request: Request<csi::v1::NodeExpandVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeExpandVolumeResponse>, Status> {
        tonic_unimplemented()
    }
}

mod pod_dir {
    use std::path::Path;

    use snafu::{OptionExt, ResultExt, Snafu};
    use stackable_operator::commons::listener::ListenerIngress;

    #[derive(Snafu, Debug)]
    pub enum Error {
        #[snafu(display("failed to write content"), context(false))]
        WriteContent { source: std::io::Error },
        #[snafu(display("listener has no address yet"))]
        NoDefaultAddress,
        #[snafu(display("default address folder is outside of the volume root"))]
        DefaultAddrIsOutsideRoot { source: std::path::StripPrefixError },
    }

    pub async fn write_listener_info_to_pod_dir(
        target_path: &Path,
        listener_addrs: &[ListenerIngress],
    ) -> Result<(), Error> {
        let addrs_path = target_path.join("addresses");
        tokio::fs::create_dir_all(&addrs_path).await?;
        let mut default_addr_dir = None;
        for addr in listener_addrs {
            let addr_dir = addrs_path.join(&addr.address);
            let ports_dir = addr_dir.join("ports");
            tokio::fs::create_dir_all(&ports_dir).await?;
            tokio::fs::write(addr_dir.join("address"), addr.address.as_bytes()).await?;
            for (port_name, port) in &addr.ports {
                tokio::fs::write(ports_dir.join(port_name), port.to_string().as_bytes()).await?;
            }
            default_addr_dir.get_or_insert(addr_dir);
        }
        tokio::fs::symlink(
            default_addr_dir
                .context(NoDefaultAddressSnafu)?
                .strip_prefix(target_path)
                .context(DefaultAddrIsOutsideRootSnafu)?,
            target_path.join("default-address"),
        )
        .await?;
        Ok(())
    }
}
