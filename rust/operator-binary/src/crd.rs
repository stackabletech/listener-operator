use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use stackable_operator::kube::CustomResource;
use stackable_operator::schemars::{self, JsonSchema};

#[cfg(doc)]
use stackable_operator::k8s_openapi::api::core::v1::{Node, Pod, Service};

/// Defines a policy for how [`Listener`]s should be exposed.
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[kube(
    group = "listeners.stackable.tech",
    version = "v1alpha1",
    kind = "ListenerClass",
    crates(
        kube_core = "stackable_operator::kube::core",
        k8s_openapi = "stackable_operator::k8s_openapi",
        schemars = "stackable_operator::schemars"
    )
)]
#[serde(rename_all = "camelCase")]
pub struct ListenerClassSpec {
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
/// Essentially a Stackable extension of a Kubernetes [`Service`]. Compared to [`Service`], [`Listener`] changes two things:
/// 1. It uses a cluster-level policy object ([`ListenerClass`]) to define how exactly the exposure works
/// 2. It has a consistent API for reading back the exposed address(es) of the service
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema, Default)]
#[kube(
    group = "listeners.stackable.tech",
    version = "v1alpha1",
    kind = "Listener",
    namespaced,
    status = "ListenerStatus",
    crates(
        kube_core = "stackable_operator::kube::core",
        k8s_openapi = "stackable_operator::k8s_openapi",
        schemars = "stackable_operator::schemars"
    )
)]
#[serde(rename_all = "camelCase")]
pub struct ListenerSpec {
    /// The name of the [`ListenerClass`].
    pub class_name: Option<String>,
    /// Labels that the [`Pod`]s must share in order to be exposed.
    pub pod_selector: Option<BTreeMap<String, String>>,
    /// Ports that should be exposed.
    pub ports: Option<Vec<ListenerPort>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListenerPort {
    /// The name of the port.
    ///
    /// The name of each port *must* be unique within a single [`Listener`].
    pub name: String,
    /// The port number.
    pub port: i32,
    /// The layer-4 protocol (`TCP` or `UDP`).
    pub protocol: Option<String>,
}

/// Informs users about how to reach the [`Listener`].
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListenerStatus {
    /// The backing Kubernetes [`Service`].
    pub service_name: Option<String>,
    /// All addresses that the [`Listener`] is currently reachable from.
    pub ingress_addresses: Option<Vec<ListenerIngress>>,
    /// Port mappings for accessing the [`Listener`] on each [`Node`] that the [`Pod`]s are currently running on.
    ///
    /// This is only intended for internal use by listener-operator itself. This will be left unset if using a [`ListenerClass`] that does
    /// not require [`Node`]-local access.
    pub node_ports: Option<BTreeMap<String, i32>>,
}

/// One address that a [`Listener`] is accessible from.
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListenerIngress {
    /// The hostname or IP address to the [`Listener`].
    pub address: String,
    /// Port mapping table.
    pub ports: BTreeMap<String, i32>,
}