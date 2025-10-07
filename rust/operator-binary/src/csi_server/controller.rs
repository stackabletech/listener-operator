use csi_grpc as csi;
use serde::{Deserialize, de::IntoDeserializer};
use snafu::{OptionExt, ResultExt, Snafu};
use stackable_operator::{
    crd::listener,
    k8s_openapi::api::core::v1::PersistentVolumeClaim,
    kube::{core::DynamicObject, runtime::reflector::ObjectRef},
};
use tonic::{Request, Response, Status};

use super::{ListenerSelector, ListenerVolumeContext, tonic_unimplemented};
use crate::utils::error::error_full_message;

pub struct ListenerOperatorController {
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
    #[snafu(display("failed to decode request parameters"))]
    DecodeRequestParams { source: serde::de::value::Error },
    #[snafu(display("failed to get {obj}"))]
    GetObject {
        source: stackable_operator::client::Error,
        obj: Box<ObjectRef<DynamicObject>>,
    },
    #[snafu(display("failed to decode volume context"))]
    DecodeVolumeContext { source: serde::de::value::Error },
    #[snafu(display("{listener} does not specify a listener class"))]
    NoListenerClass {
        listener: ObjectRef<listener::v1alpha1::Listener>,
    },
}

impl From<CreateVolumeError> for Status {
    fn from(err: CreateVolumeError) -> Self {
        let full_msg = error_full_message(&err);
        // Convert to an appropriate tonic::Status representation and include full error message
        match err {
            CreateVolumeError::DecodeRequestParams { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::DecodeVolumeContext { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::NoListenerClass { .. } => Status::invalid_argument(full_msg),
            CreateVolumeError::GetObject { .. } => Status::unavailable(full_msg),
        }
    }
}

#[tonic::async_trait]
impl csi::v1::controller_server::Controller for ListenerOperatorController {
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
        use create_volume_error::*;
        let request = request.into_inner();
        let ControllerVolumeParams {
            pvc_name,
            pvc_namespace: ns,
        } = ControllerVolumeParams::deserialize(request.parameters.into_deserializer())
            .context(create_volume_error::DecodeRequestParamsSnafu)?;
        let pvc = self
            .client
            .get::<PersistentVolumeClaim>(&pvc_name, &ns)
            .await
            .with_context(|_| GetObjectSnafu {
                obj: ObjectRef::<PersistentVolumeClaim>::new(&pvc_name)
                    .within(&ns)
                    .erase(),
            })?;
        let raw_volume_context = pvc.metadata.annotations.unwrap_or_default();
        let ListenerVolumeContext { listener_selector } =
            ListenerVolumeContext::deserialize(raw_volume_context.clone().into_deserializer())
                .context(create_volume_error::DecodeVolumeContextSnafu)?;
        let listener_class_name = match listener_selector {
            ListenerSelector::Listener(listener_name) => {
                let listener = self
                    .client
                    .get::<listener::v1alpha1::Listener>(&listener_name, &ns)
                    .await
                    .with_context(|_| GetObjectSnafu {
                        obj: ObjectRef::<listener::v1alpha1::Listener>::new(&listener_name)
                            .within(&ns)
                            .erase(),
                    })?;
                listener
                    .spec
                    .class_name
                    .clone()
                    .with_context(|| NoListenerClassSnafu {
                        listener: ObjectRef::from_obj(&listener),
                    })?
            }
            ListenerSelector::ListenerClass(listener_class) => listener_class,
        };
        let listener_class = self
            .client
            .get::<listener::v1alpha1::ListenerClass>(&listener_class_name, &())
            .await
            .with_context(|_| GetObjectSnafu {
                obj: ObjectRef::<listener::v1alpha1::ListenerClass>::new(&listener_class_name)
                    .within(&ns)
                    .erase(),
            })?;

        // We only configure a node stickiness in case it is enabled and the Service is of type
        // NodePort.
        let accessible_topology = if listener_class.spec.sticky_node_ports
            && listener_class.spec.service_type == listener::v1alpha1::ServiceType::NodePort
        {
            // Pick the top node (as selected by the CSI client) and "stick" to that
            // Since we want clients to have a stable address to connect to
            request
                .accessibility_requirements
                .unwrap_or_default()
                .preferred
                .into_iter()
                .take(1)
                .collect()
        } else {
            Vec::new()
        };

        Ok(Response::new(csi::v1::CreateVolumeResponse {
            volume: Some(csi::v1::Volume {
                capacity_bytes: 0,
                volume_id: request.name,
                volume_context: raw_volume_context.into_iter().collect(),
                content_source: None,
                accessible_topology,
            }),
        }))
    }

    async fn delete_volume(
        &self,
        _request: Request<csi::v1::DeleteVolumeRequest>,
    ) -> Result<Response<csi::v1::DeleteVolumeResponse>, Status> {
        Ok(Response::new(csi::v1::DeleteVolumeResponse {}))
    }

    async fn controller_publish_volume(
        &self,
        _request: Request<csi::v1::ControllerPublishVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerPublishVolumeResponse>, Status> {
        tonic_unimplemented()
    }

    async fn controller_unpublish_volume(
        &self,
        _request: Request<csi::v1::ControllerUnpublishVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerUnpublishVolumeResponse>, Status> {
        tonic_unimplemented()
    }

    async fn validate_volume_capabilities(
        &self,
        _request: Request<csi::v1::ValidateVolumeCapabilitiesRequest>,
    ) -> Result<Response<csi::v1::ValidateVolumeCapabilitiesResponse>, Status> {
        tonic_unimplemented()
    }

    async fn list_volumes(
        &self,
        _request: Request<csi::v1::ListVolumesRequest>,
    ) -> Result<Response<csi::v1::ListVolumesResponse>, Status> {
        tonic_unimplemented()
    }

    async fn get_capacity(
        &self,
        _request: Request<csi::v1::GetCapacityRequest>,
    ) -> Result<Response<csi::v1::GetCapacityResponse>, Status> {
        tonic_unimplemented()
    }

    async fn create_snapshot(
        &self,
        _request: Request<csi::v1::CreateSnapshotRequest>,
    ) -> Result<Response<csi::v1::CreateSnapshotResponse>, Status> {
        tonic_unimplemented()
    }

    async fn delete_snapshot(
        &self,
        _request: Request<csi::v1::DeleteSnapshotRequest>,
    ) -> Result<Response<csi::v1::DeleteSnapshotResponse>, Status> {
        tonic_unimplemented()
    }

    async fn list_snapshots(
        &self,
        _request: Request<csi::v1::ListSnapshotsRequest>,
    ) -> Result<Response<csi::v1::ListSnapshotsResponse>, Status> {
        tonic_unimplemented()
    }

    async fn controller_expand_volume(
        &self,
        _request: Request<csi::v1::ControllerExpandVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerExpandVolumeResponse>, Status> {
        tonic_unimplemented()
    }

    async fn controller_get_volume(
        &self,
        _request: Request<csi::v1::ControllerGetVolumeRequest>,
    ) -> Result<Response<csi::v1::ControllerGetVolumeResponse>, Status> {
        tonic_unimplemented()
    }
}
