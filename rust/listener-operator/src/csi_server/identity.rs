use std::collections::HashMap;

use clap::crate_version;
use tonic::{Request, Response, Status};

use crate::grpc::csi;

pub struct LbOperatorIdentity;

#[tonic::async_trait]
impl csi::v1::identity_server::Identity for LbOperatorIdentity {
    async fn get_plugin_info(
        &self,
        _request: Request<csi::v1::GetPluginInfoRequest>,
    ) -> Result<Response<csi::v1::GetPluginInfoResponse>, Status> {
        Ok(Response::new(csi::v1::GetPluginInfoResponse {
            name: "lb.stackable.tech".to_string(),
            vendor_version: crate_version!().to_string(),
            manifest: HashMap::new(),
        }))
    }

    async fn get_plugin_capabilities(
        &self,
        _request: Request<csi::v1::GetPluginCapabilitiesRequest>,
    ) -> Result<Response<csi::v1::GetPluginCapabilitiesResponse>, Status> {
        Ok(Response::new(csi::v1::GetPluginCapabilitiesResponse {
            capabilities: vec![
                csi::v1::PluginCapability {
                    r#type: Some(csi::v1::plugin_capability::Type::Service(
                        csi::v1::plugin_capability::Service {
                            r#type:
                                csi::v1::plugin_capability::service::Type::VolumeAccessibilityConstraints
                                    .into(),
                        },
                    )),
                },
                csi::v1::PluginCapability {
                    r#type: Some(csi::v1::plugin_capability::Type::Service(
                        csi::v1::plugin_capability::Service {
                            r#type: csi::v1::plugin_capability::service::Type::ControllerService.into(),
                        },
                    )),
                },
            ],
        }))
    }

    async fn probe(
        &self,
        _request: Request<csi::v1::ProbeRequest>,
    ) -> Result<Response<csi::v1::ProbeResponse>, Status> {
        Ok(Response::new(csi::v1::ProbeResponse { ready: Some(true) }))
    }
}
