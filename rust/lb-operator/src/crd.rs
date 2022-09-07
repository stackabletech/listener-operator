use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stackable_operator::kube::CustomResource;
use stackable_operator::schemars::{self, JsonSchema};

#[cfg(rustdoc)]
use stackable_operator::k8s_openapi::api::core::v1::{Pod, Service};

/// Defines a policy for how [`LoadBalancer`]s should be exposed.
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
    /// Annotations that should be added to the [`Service`] object.
    #[serde(default)]
    pub service_annotations: BTreeMap<String, String>,
}

/// The method used to access the services.
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema, PartialEq, Eq)]
pub enum ServiceType {
    /// Reserve a port on each node.
    NodePort,
    /// Provision a dedicated load balancer.
    LoadBalancer,
}

/// Exposes a set of pods to the outside world.
///
/// Essentially a Stackable extension of a Kubernetes [`Service`]. Compared to [`Service`], [`LoadBalancer`] changes two things:
/// 1. It uses a cluster-level policy object ([`LoadBalancerClass`]) to define how exactly the exposure works
/// 2. It has a consistent API for reading back the exposed address(es) of the service
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
    /// The name of the [`LoadBalancerClass`].
    pub class_name: Option<String>,
    /// Labels that the [`Pod`]s must share in order to be exposed.
    pub pod_selector: Option<BTreeMap<String, String>>,
    /// Ports that should be exposed.
    pub ports: Option<Vec<LoadBalancerPort>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerPort {
    /// The name of the port.
    ///
    /// The name of each port *must* be unique within a single [`LoadBalancer`].
    pub name: String,
    /// The port number.
    pub port: i32,
    /// The layer-4 protocol (`TCP` or `UDP`).
    pub protocol: Option<String>,
}

/// Informs users about how to reach the [`LoadBalancer`].
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerStatus {
    /// The backing Kubernetes [`Service`].
    pub service_name: Option<String>,
    /// All addresses that the [`LoadBalancer`] is currently reachable from.
    pub ingress_addresses: Option<Vec<LoadBalancerIngress>>,
    /// Port mappings for accessing the [`LoadBalancer`] on each [`Node`] that the [`Pod`]s are currently running on.
    ///
    /// This is only intended for internal use by lb-operator itself. This will be left unset if using a [`LoadBalancerClass`] that does
    /// not require [`Node`]-local access.
    pub node_ports: Option<BTreeMap<String, i32>>,
}

/// One address that a [`LoadBalancer`] is accessible from.
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerIngress {
    /// The hostname or IP address to the [`LoadBalancer`].
    pub address: String,
    /// Port mapping table.
    pub ports: BTreeMap<String, i32>,
}
