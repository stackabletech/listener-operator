use serde::{de::IntoDeserializer, Deserialize};
use stackable_operator::k8s_openapi::api::core::v1::PersistentVolumeClaim;
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancer, LoadBalancerClass, ServiceType},
    grpc::csi,
};

use super::{LbSelector, LbVolumeContext};

pub struct LbOperatorController {
    pub client: stackable_operator::client::Client,
}

#[derive(Deserialize)]
struct ControllerVolumeParams {
    #[serde(rename = "csi.storage.k8s.io/pvc/name")]
    pvc_name: String,
    #[serde(rename = "csi.storage.k8s.io/pvc/namespace")]
    pvc_namespace: String,
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
        let ControllerVolumeParams {
            pvc_name,
            pvc_namespace: ns,
        } = ControllerVolumeParams::deserialize(request.parameters.into_deserializer())
            .map_err(|e: serde::de::value::Error| e)
            .unwrap();
        let pvc = self
            .client
            .get::<PersistentVolumeClaim>(&pvc_name, Some(&ns))
            .await
            .unwrap();
        let raw_volume_context = pvc.metadata.annotations.unwrap_or_default();
        let LbVolumeContext { lb_selector } =
            LbVolumeContext::deserialize(raw_volume_context.clone().into_deserializer())
                .map_err(|e: serde::de::value::Error| e)
                .unwrap();
        let lb_class_name = match lb_selector {
            LbSelector::Lb(lb_name) => self
                .client
                .get::<LoadBalancer>(&lb_name, Some(&ns))
                .await
                .unwrap()
                .spec
                .class_name
                .unwrap(),
            LbSelector::LbClass(lb_class) => lb_class,
        };
        let lb_class = self
            .client
            .get::<LoadBalancerClass>(&lb_class_name, None)
            .await
            .unwrap();
        Ok(Response::new(csi::v1::CreateVolumeResponse {
            volume: Some(csi::v1::Volume {
                capacity_bytes: 0,
                volume_id: request.name,
                volume_context: raw_volume_context.into_iter().collect(),
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
