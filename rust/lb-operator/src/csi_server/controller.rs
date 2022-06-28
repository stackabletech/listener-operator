use stackable_operator::k8s_openapi::api::core::v1::PersistentVolumeClaim;
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancerClass, ServiceType},
    grpc::csi,
};

pub struct LbOperatorController {
    pub client: stackable_operator::client::Client,
}

#[tonic::async_trait]
impl csi::v1::controller_server::Controller for LbOperatorController {
    async fn controller_get_capabilities(
        &self,
        _request: Request<csi::v1::ControllerGetCapabilitiesRequest>,
    ) -> Result<Response<csi::v1::ControllerGetCapabilitiesResponse>, Status> {
        Ok(Response::new(csi::v1::ControllerGetCapabilitiesResponse {
            capabilities: vec![csi::v1::ControllerServiceCapability {
                r#type: Some(csi::v1::controller_service_capability::Type::Rpc(
                    csi::v1::controller_service_capability::Rpc {
                        r#type:
                            csi::v1::controller_service_capability::rpc::Type::CreateDeleteVolume
                                .into(),
                    },
                )),
            }],
        }))
    }

    async fn create_volume(
        &self,
        request: Request<csi::v1::CreateVolumeRequest>,
    ) -> Result<Response<csi::v1::CreateVolumeResponse>, Status> {
        let request = request.into_inner();
        let pvc = self
            .client
            .get::<PersistentVolumeClaim>(
                request
                    .parameters
                    .get("csi.storage.k8s.io/pvc/name")
                    .unwrap(),
                Some(
                    request
                        .parameters
                        .get("csi.storage.k8s.io/pvc/namespace")
                        .unwrap(),
                ),
            )
            .await
            .unwrap();
        let volume_context = pvc.metadata.annotations.unwrap_or_default();
        let lb_class = self
            .client
            .get::<LoadBalancerClass>(
                volume_context.get("lb.stackable.tech/lb-class").unwrap(),
                None,
            )
            .await
            .unwrap();
        Ok(Response::new(csi::v1::CreateVolumeResponse {
            volume: Some(csi::v1::Volume {
                capacity_bytes: 0,
                volume_id: request.name,
                volume_context: volume_context.into_iter().collect(),
                content_source: None,
                accessible_topology: match lb_class.spec.service_type {
                    ServiceType::NodePort => vec![request
                        .accessibility_requirements
                        .unwrap_or_default()
                        .preferred
                        .first()
                        .unwrap()
                        .clone()],
                    ServiceType::LoadBalancer => Vec::new(),
                },
            }),
        }))
    }

    async fn delete_volume(
        &self,
        request: Request<csi::v1::DeleteVolumeRequest>,
    ) -> Result<Response<csi::v1::DeleteVolumeResponse>, Status> {
        Ok(Response::new(csi::v1::DeleteVolumeResponse {}))
    }

    async fn controller_publish_volume(
        &self,
        request: Request<csi::v1::ControllerPublishVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerPublishVolumeResponse>, Status> {
        todo!()
    }

    async fn controller_unpublish_volume(
        &self,
        request: Request<csi::v1::ControllerUnpublishVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerUnpublishVolumeResponse>, Status> {
        todo!()
    }

    async fn validate_volume_capabilities(
        &self,
        request: Request<csi::v1::ValidateVolumeCapabilitiesRequest>,
    ) -> Result<Response<csi::v1::ValidateVolumeCapabilitiesResponse>, Status> {
        todo!()
    }

    async fn list_volumes(
        &self,
        request: Request<csi::v1::ListVolumesRequest>,
    ) -> Result<Response<csi::v1::ListVolumesResponse>, Status> {
        todo!()
    }

    async fn get_capacity(
        &self,
        request: Request<csi::v1::GetCapacityRequest>,
    ) -> Result<Response<csi::v1::GetCapacityResponse>, Status> {
        todo!()
    }

    async fn create_snapshot(
        &self,
        request: Request<csi::v1::CreateSnapshotRequest>,
    ) -> Result<Response<csi::v1::CreateSnapshotResponse>, Status> {
        todo!()
    }

    async fn delete_snapshot(
        &self,
        request: Request<csi::v1::DeleteSnapshotRequest>,
    ) -> Result<Response<csi::v1::DeleteSnapshotResponse>, Status> {
        todo!()
    }

    async fn list_snapshots(
        &self,
        request: Request<csi::v1::ListSnapshotsRequest>,
    ) -> Result<Response<csi::v1::ListSnapshotsResponse>, Status> {
        todo!()
    }

    async fn controller_expand_volume(
        &self,
        request: Request<csi::v1::ControllerExpandVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerExpandVolumeResponse>, Status> {
        todo!()
    }

    async fn controller_get_volume(
        &self,
        request: Request<csi::v1::ControllerGetVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerGetVolumeResponse>, Status> {
        todo!()
    }
}
