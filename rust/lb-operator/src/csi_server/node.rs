use std::path::PathBuf;

use stackable_operator::{
    builder::OwnerReferenceBuilder,
    k8s_openapi::api::core::v1::{Node, PersistentVolume, Pod, Service, ServicePort, ServiceSpec},
    kube::core::ObjectMeta,
};
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancerClass, ServiceType},
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
        let lb_class = self
            .client
            .get::<LoadBalancerClass>(
                request
                    .volume_context
                    .get("lb.stackable.tech/lb-class")
                    .unwrap(),
                None,
            )
            .await
            .unwrap();
        let pv = self
            .client
            .get::<PersistentVolume>(&request.volume_id, None)
            .await
            .unwrap();
        let pod = self.client.get::<Pod>(pod_name, Some(ns)).await.unwrap();
        let node = self
            .client
            .get::<Node>(
                pod.spec
                    .as_ref()
                    .and_then(|ps| ps.node_name.as_deref())
                    .unwrap(),
                None,
            )
            .await
            .unwrap();
        let svc = Service {
            metadata: ObjectMeta {
                namespace: Some(ns.clone()),
                name: Some(request.volume_id.clone()),
                owner_references: Some(vec![OwnerReferenceBuilder::new()
                    .initialize_from_resource(&pv)
                    .build()
                    .unwrap()]),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some(match lb_class.spec.service_type {
                    ServiceType::NodePort => "NodePort".to_string(),
                    ServiceType::LoadBalancer => "LoadBalancer".to_string(),
                }),
                ports: Some(
                    pod.spec
                        .iter()
                        .flat_map(|ps| &ps.containers)
                        .flat_map(|ctr| &ctr.ports)
                        .flatten()
                        .map(|port| ServicePort {
                            name: port.name.clone(),
                            protocol: port.protocol.clone(),
                            port: port.container_port,
                            ..Default::default()
                        })
                        .collect(),
                ),
                external_traffic_policy: Some("Local".to_string()),
                selector: pod.metadata.labels,
                ..Default::default()
            }),
            ..Default::default()
        };
        let svc = self
            .client
            .apply_patch(FIELD_MANAGER_SCOPE, &svc, &svc)
            .await
            .unwrap();

        let address;
        let ports: Vec<_>;
        match lb_class.spec.service_type {
            ServiceType::NodePort => {
                address = node.metadata.name.as_deref().unwrap();
                ports = svc
                    .spec
                    .as_ref()
                    .unwrap()
                    .ports
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|port| (port.name.as_deref().unwrap(), port.node_port.unwrap()))
                    .collect();
            }
            ServiceType::LoadBalancer => {
                address = svc
                    .status
                    .as_ref()
                    .and_then(|ss| {
                        let ingress = ss.load_balancer.as_ref()?.ingress.as_ref()?.first()?;
                        ingress.hostname.as_deref().or(ingress.ip.as_deref())
                    })
                    .unwrap();
                ports = svc
                    .spec
                    .as_ref()
                    .unwrap()
                    .ports
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|port| (port.name.as_deref().unwrap(), port.port))
                    .collect();
            }
        };

        let target_path = PathBuf::from(&request.target_path);
        let ports_path = target_path.join("ports");
        tokio::fs::create_dir_all(&ports_path).await.unwrap();
        tokio::fs::File::create(target_path.join("address"))
            .await
            .unwrap()
            .write_all(address.as_bytes())
            .await
            .unwrap();
        for (port_name, port) in ports {
            tokio::fs::File::create(ports_path.join(port_name))
                .await
                .unwrap()
                .write_all(port.to_string().as_bytes())
                .await
                .unwrap();
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
