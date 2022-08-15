use serde::{de::IntoDeserializer, Deserialize};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    k8s_openapi::api::core::v1::PersistentVolumeClaim, kube::runtime::reflector::ObjectRef,
};
use tonic::{Request, Response, Status};

use crate::{
    crd::{LoadBalancer, LoadBalancerClass, ServiceType},
    grpc::csi,
    utils::error_full_message,
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

#[derive(Snafu, Debug)]
#[snafu(module)]
enum CreateVolumeError {
    DecodeRequestParams {
        source: serde::de::value::Error,
    },
    GetPvc {
        source: stackable_operator::error::Error,
        pvc: ObjectRef<PersistentVolumeClaim>,
    },
    DecodeVolumeContext {
        source: serde::de::value::Error,
    },
    GetLoadBalancer {
        source: stackable_operator::error::Error,
        lb: ObjectRef<LoadBalancer>,
    },
    NoLoadBalancerClass {
        lb: ObjectRef<LoadBalancer>,
    },
    GetLoadBalancerClass {
        source: stackable_operator::error::Error,
        lb_class: ObjectRef<LoadBalancerClass>,
    },
}

impl From<CreateVolumeError> for Status {
    fn from(err: CreateVolumeError) -> Self {
        let full_msg = error_full_message(&err);
        // Convert to an appropriate tonic::Status representation and include full error message
        match err {
            CreateVolumeError::DecodeRequestParams { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::DecodeVolumeContext { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::NoLoadBalancerClass { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::GetPvc { .. } => Status::unavailable(full_msg),
            CreateVolumeError::GetLoadBalancer { .. } => Status::unavailable(full_msg),
            CreateVolumeError::GetLoadBalancerClass { .. } => Status::unavailable(full_msg),
        }
    }
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
            .context(create_volume_error::DecodeRequestParamsSnafu)?;
        let pvc = self
            .client
            .get::<PersistentVolumeClaim>(&pvc_name, Some(&ns))
            .await
            .with_context(|_| create_volume_error::GetPvcSnafu {
                pvc: ObjectRef::new(&pvc_name).within(&ns),
            })?;
        let raw_volume_context = pvc.metadata.annotations.unwrap_or_default();
        let LbVolumeContext { lb_selector } =
            LbVolumeContext::deserialize(raw_volume_context.clone().into_deserializer())
                .context(create_volume_error::DecodeVolumeContextSnafu)?;
        let lb_class_name = match lb_selector {
            LbSelector::Lb(lb_name) => {
                let lb = self
                    .client
                    .get::<LoadBalancer>(&lb_name, Some(&ns))
                    .await
                    .with_context(|_| create_volume_error::GetLoadBalancerSnafu {
                        lb: ObjectRef::new(&lb_name).within(&ns),
                    })?;
                lb.spec.class_name.clone().with_context(|| {
                    create_volume_error::NoLoadBalancerClassSnafu {
                        lb: ObjectRef::from_obj(&lb),
                    }
                })?
            }
            LbSelector::LbClass(lb_class) => lb_class,
        };
        let lb_class = self
            .client
            .get::<LoadBalancerClass>(&lb_class_name, None)
            .await
            .with_context(|_| create_volume_error::GetLoadBalancerClassSnafu {
                lb_class: ObjectRef::new(&lb_class_name).within(&ns),
            })?;
        Ok(Response::new(csi::v1::CreateVolumeResponse {
            volume: Some(csi::v1::Volume {
                capacity_bytes: 0,
                volume_id: request.name,
                volume_context: raw_volume_context.into_iter().collect(),
                content_source: None,
                accessible_topology: match lb_class.spec.service_type {
                    ServiceType::NodePort => request
                        .accessibility_requirements
                        .unwrap_or_default()
                        .preferred
                        .into_iter()
                        .take(1)
                        .collect(),
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
