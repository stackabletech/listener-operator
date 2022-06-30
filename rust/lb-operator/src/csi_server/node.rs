use std::path::PathBuf;

use stackable_operator::{
    builder::OwnerReferenceBuilder,
    k8s_openapi::api::core::v1::{Node, PersistentVolume, Pod, Service, ServicePort, ServiceSpec},
    kube::core::ObjectMeta,
};
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancer, LoadBalancerClass, LoadBalancerPort, LoadBalancerSpec, ServiceType},
    grpc::csi::{self, v1::Topology},
};

const FIELD_MANAGER_SCOPE: &str = "volume";

pub struct LbOperatorNode {
    pub client: stackable_operator::client::Client,
    pub node_name: String,
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
        let request = request.into_inner();
        let ns = request
            .volume_context
            .get("csi.storage.k8s.io/pod.namespace")
            .unwrap();
        let pod_name = request
            .volume_context
            .get("csi.storage.k8s.io/pod.name")
            .unwrap();
        let pv = self
            .client
            .get::<PersistentVolume>(&request.volume_id, None)
            .await
            .unwrap();
        let pod = self.client.get::<Pod>(pod_name, Some(ns)).await.unwrap();

        let lb = if let Some(lb_name) = request.volume_context.get("lb.stackable.tech/lb-name") {
            self.client.get(lb_name, Some(ns)).await.unwrap()
        } else {
            let lb = LoadBalancer {
                metadata: ObjectMeta {
                    namespace: Some(ns.clone()),
                    name: Some(request.volume_id.clone()),
                    owner_references: Some(vec![OwnerReferenceBuilder::new()
                        .initialize_from_resource(&pv)
                        .build()
                        .unwrap()]),
                    ..Default::default()
                },
                spec: LoadBalancerSpec {
                    class_name: request
                        .volume_context
                        .get("lb.stackable.tech/lb-class")
                        .cloned(),
                    ports: Some(
                        pod.spec
                            .iter()
                            .flat_map(|ps| &ps.containers)
                            .flat_map(|ctr| &ctr.ports)
                            .flatten()
                            .map(|port| LoadBalancerPort {
                                name: port.name.clone().unwrap(),
                                protocol: port.protocol.clone(),
                                port: port.container_port,
                            })
                            .collect(),
                    ),
                    pod_selector: pod.metadata.labels,
                },
                status: None,
            };
            self.client
                .apply_patch(FIELD_MANAGER_SCOPE, &lb, &lb)
                .await
                .unwrap()
        };

        let target_path = PathBuf::from(&request.target_path);
        let addrs_path = target_path.join("addresses");
        tokio::fs::create_dir_all(&addrs_path).await.unwrap();
        for addr in lb
            .status
            .as_ref()
            .and_then(|lbs| lbs.ingress_addresses.as_ref())
            .into_iter()
            .flatten()
        {
            let addr_dir = addrs_path.join(&addr.address);
            let ports_dir = addr_dir.join("ports");
            tokio::fs::create_dir_all(&ports_dir).await.unwrap();
            tokio::fs::write(addr_dir.join("address"), addr.address.as_bytes())
                .await
                .unwrap();
            for (port_name, port) in &addr.ports {
                tokio::fs::write(ports_dir.join(port_name), port.to_string().as_bytes())
                    .await
                    .unwrap();
            }
        }

        Ok(Response::new(csi::v1::NodePublishVolumeResponse {}))
    }

    async fn node_unpublish_volume(
        &self,
        request: Request<csi::v1::NodeUnpublishVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeUnpublishVolumeResponse>, Status> {
        let request = request.into_inner();
        match tokio::fs::remove_dir_all(request.target_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // already deleted => nothing to do
            }
            Err(err) => Err(err).unwrap(),
        }
        Ok(Response::new(csi::v1::NodeUnpublishVolumeResponse {}))
    }

    async fn node_stage_volume(
        &self,
        request: Request<csi::v1::NodeStageVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeStageVolumeResponse>, Status> {
        todo!()
    }

    async fn node_unstage_volume(
        &self,
        request: Request<csi::v1::NodeUnstageVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeUnstageVolumeResponse>, Status> {
        todo!()
    }

    async fn node_get_volume_stats(
        &self,
        request: Request<csi::v1::NodeGetVolumeStatsRequest>,
    ) -> Result<Response<csi::v1::NodeGetVolumeStatsResponse>, Status> {
        todo!()
    }

    async fn node_expand_volume(
        &self,
        request: Request<csi::v1::NodeExpandVolumeRequest>,
    ) -> Result<Response<csi::v1::NodeExpandVolumeResponse>, Status> {
        todo!()
    }
}
