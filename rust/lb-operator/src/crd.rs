use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stackable_operator::kube::CustomResource;
use stackable_operator::schemars::{self, JsonSchema};

#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "lb.stackable.tech",
    version = "v1alpha1",
    kind = "LoadBalancerClass",
    crates(
        kube_core = "stackable_operator::kube::core",
        k8s_openapi = "stackable_operator::k8s_openapi",
        schemars = "stackable_operator::schemars"
    )
)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerClassSpec {
    pub service_type: ServiceType,
    #[serde(default)]
    pub service_annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub enum ServiceType {
    NodePort,
    LoadBalancer,
}
