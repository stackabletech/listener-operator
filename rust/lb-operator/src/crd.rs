use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stackable_operator::kube::CustomResource;
use stackable_operator::schemars::{self, JsonSchema};

#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "lb.stackable.tech",
    version = "v1alpha1",
    kind = "LoadBalancerClass",
    shortname = "lbclass",
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

#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema, Default)]
#[kube(
    group = "lb.stackable.tech",
    version = "v1alpha1",
    kind = "LoadBalancer",
    shortname = "lb",
    namespaced,
    status = "LoadBalancerStatus",
    crates(
        kube_core = "stackable_operator::kube::core",
        k8s_openapi = "stackable_operator::k8s_openapi",
        schemars = "stackable_operator::schemars"
    )
)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerSpec {
    pub class_name: Option<String>,
    pub pod_selector: Option<BTreeMap<String, String>>,
    pub ports: Option<Vec<LoadBalancerPort>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerPort {
    pub name: String,
    pub port: i32,
    pub protocol: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerStatus {
    pub service_name: Option<String>,
    pub ingress_addresses: Option<Vec<LoadBalancerIngress>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerIngress {
    pub address: String,
    pub ports: BTreeMap<String, i32>,
}
