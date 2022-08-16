use std::{fmt::Debug, path::PathBuf};

use serde::{
    de::{DeserializeOwned, IntoDeserializer},
    Deserialize,
};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    builder::OwnerReferenceBuilder,
    k8s_openapi::api::core::v1::{PersistentVolume, Pod},
    kube::{
        core::{DynamicObject, ObjectMeta},
        runtime::reflector::ObjectRef,
        Resource,
    },
};
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancer, LoadBalancerIngress, LoadBalancerPort, LoadBalancerSpec},
    grpc::csi::{self, v1::Topology},
    utils::error_full_message,
};

use super::{tonic_unimplemented, LbSelector, LbVolumeContext};

const FIELD_MANAGER_SCOPE: &str = "volume";

pub struct LbOperatorNode {
    pub client: stackable_operator::client::Client,
    pub node_name: String,
}

#[derive(Deserialize)]
struct LbNodeVolumeContext {
    #[serde(rename = "csi.storage.k8s.io/pod.namespace")]
    pod_namespace: String,
    #[serde(rename = "csi.storage.k8s.io/pod.name")]
    pod_name: String,

    #[serde(flatten)]
    common: LbVolumeContext,
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
    #[snafu(display("failed to build LoadBalancer's owner reference"))]
    BuildLbOwnerRef {
        source: stackable_operator::error::Error,
    },
    #[snafu(display("failed to apply {lb}"))]
    ApplyLb {
        source: stackable_operator::error::Error,
        lb: ObjectRef<LoadBalancer>,
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
            PublishVolumeError::BuildLbOwnerRef { .. } => Status::unavailable(full_msg),
            PublishVolumeError::ApplyLb { .. } => Status::unavailable(full_msg),
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
impl csi::v1::node_server::Node for LbOperatorNode {
    async fn node_get_info(
        &self,
        _request: Request<csi::v1::NodeGetInfoRequest>,
    ) -> Result<Response<csi::v1::NodeGetInfoResponse>, Status> {
        Ok(Response::new(csi::v1::NodeGetInfoResponse {
            node_id: self.node_name.clone(),
            max_volumes_per_node: i64::MAX,
            accessible_topology: Some(Topology {
                segments: [(
                    "lb.stackable.tech/hostname".to_string(),
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
        async fn get_obj<K: Resource<DynamicType = ()> + DeserializeOwned + Clone + Debug>(
            client: &stackable_operator::client::Client,
            name: &str,
            ns: Option<&str>,
        ) -> Result<K, PublishVolumeError> {
            client.get(name, ns).await.with_context(|_| GetObjectSnafu {
                obj: {
                    let mut obj = ObjectRef::<K>::new(name);
                    if let Some(ns) = ns {
                        obj = obj.within(ns);
                    }
                    obj.erase()
                },
            })
        }
        // let get_obj = |name: &str, ns: Option<&str>| self.client.get(name, ns);

        let request = request.into_inner();
        let LbNodeVolumeContext {
            pod_namespace: ns,
            pod_name,
            common: LbVolumeContext { lb_selector },
        } = LbNodeVolumeContext::deserialize(request.volume_context.into_deserializer())
            .context(DecodeVolumeContextSnafu)?;
        let pv_name = &request.volume_id;
        let pv = get_obj::<PersistentVolume>(&self.client, pv_name, None).await?;
        let pod = get_obj::<Pod>(&self.client, &pod_name, Some(&ns)).await?;

        let lb = match lb_selector {
            LbSelector::Lb(lb_name) => {
                get_obj::<LoadBalancer>(&self.client, &lb_name, Some(&ns)).await?
            }
            LbSelector::LbClass(lb_class_name) => {
                let lb = LoadBalancer {
                    metadata: ObjectMeta {
                        namespace: Some(ns.clone()),
                        name: pv
                            .spec
                            .as_ref()
                            .and_then(|pv_spec| pv_spec.claim_ref.as_ref()?.name.clone()),
                        owner_references: Some(vec![OwnerReferenceBuilder::new()
                            .initialize_from_resource(&pv)
                            .build()
                            .context(BuildLbOwnerRefSnafu)?]),
                        ..Default::default()
                    },
                    spec: LoadBalancerSpec {
                        class_name: Some(lb_class_name),
                        ports: Some(
                            pod.spec
                                .iter()
                                .flat_map(|ps| &ps.containers)
                                .flat_map(|ctr| &ctr.ports)
                                .flatten()
                                .map(|port| LoadBalancerPort {
                                    name: port
                                        .name
                                        .clone()
                                        .unwrap_or_else(|| format!("port-{}", port.container_port)),
                                    protocol: port.protocol.clone(),
                                    port: port.container_port,
                                })
                                .collect(),
                        ),
                        pod_selector: pod.metadata.labels.clone(),
                    },
                    status: None,
                };
                self.client
                    .apply_patch(FIELD_MANAGER_SCOPE, &lb, &lb)
                    .await
                    .with_context(|_| ApplyLbSnafu {
                        lb: ObjectRef::from_obj(&lb),
                    })?
            }
        };

        // Prefer calculating a per-node address where possible, to ensure that the address at least tries to
        // connect to the pod in question.
        // We also can't rely on `ingress_addresses` being set yet, since the pod won't not have an IP address yet
        // (and so can't be found in `Endpoints`)
        let lb_addrs = if let Some(node_ports) = lb
            .status
            .as_ref()
            .and_then(|status| status.node_ports.clone())
        {
            vec![LoadBalancerIngress {
                address: pod
                    .spec
                    .as_ref()
                    .and_then(|s| s.node_name.clone())
                    .with_context(|| PodHasNoNodeSnafu {
                        pod: ObjectRef::from_obj(&pod),
                    })?,
                ports: node_ports,
            }]
        } else {
            lb.status
                .as_ref()
                .and_then(|lbs| lbs.ingress_addresses.as_ref())
                .cloned()
                .unwrap_or_default()
        };

        let target_path = PathBuf::from(request.target_path);
        pod_dir::write_lb_info_to_pod_dir(&target_path, &lb_addrs)
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

    use crate::crd::LoadBalancerIngress;
    use snafu::{OptionExt, ResultExt, Snafu};

    #[derive(Snafu, Debug)]
    pub enum Error {
        #[snafu(context(false))]
        Fs { source: std::io::Error },
        #[snafu(display("load balancer has no address yet"))]
        NoDefaultLb,
        #[snafu(display("default address folder is outside of the volume root"))]
        DefaultAddrIsOutsideRoot { source: std::path::StripPrefixError },
    }

    pub async fn write_lb_info_to_pod_dir(
        target_path: &Path,
        lb_addrs: &[LoadBalancerIngress],
    ) -> Result<(), Error> {
        let addrs_path = target_path.join("addresses");
        tokio::fs::create_dir_all(&addrs_path).await?;
        let mut default_addr_dir = None;
        for addr in lb_addrs {
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
                .context(NoDefaultLbSnafu)?
                .strip_prefix(&target_path)
                .context(DefaultAddrIsOutsideRootSnafu)?,
            target_path.join("default-address"),
        )
        .await?;
        Ok(())
    }
}
