use serde_json::json;
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use stackable_operator::{
    crd::listener::v1alpha1::{Listener, OpenShiftRouteConfig, OpenShiftRouteTls},
    k8s_openapi::api::core::v1::Service,
    kube::{
        self, Api, ResourceExt,
        api::{ApiResource, DeleteParams, DynamicObject, Patch, PatchParams, Preconditions},
        core::GroupVersionKind,
        discovery,
    },
};

use crate::listener_controller::is_owned_by_listener;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display(
        "the Listener has {count} ports, so the ListenerClass must select one using openshiftRoute.port"
    ))]
    AmbiguousPort { count: usize },

    #[snafu(display("the Listener has no TCP port named {port:?}"))]
    NoTcpPort { port: String },

    #[snafu(display("Route has no namespace"))]
    NoNamespace,

    #[snafu(display("failed to look up pre-existing Route {name:?}"))]
    GetRoute { source: kube::Error, name: String },

    #[snafu(display(
        "refusing to overwrite pre-existing Route {name:?} that is not owned by this Listener"
    ))]
    RefuseToOverwriteForeignRoute { name: String },

    #[snafu(display("failed to apply Route {name:?}"))]
    ApplyRoute { source: kube::Error, name: String },

    #[snafu(display("failed to delete Route {name:?}"))]
    DeleteRoute { source: kube::Error, name: String },
}
type Result<T, E = Error> = std::result::Result<T, E>;

fn gvk() -> GroupVersionKind {
    GroupVersionKind::gvk("route.openshift.io", "v1", "Route")
}

pub async fn discover(client: kube::Client) -> Result<Option<ApiResource>, kube::Error> {
    match discovery::pinned_kind(&client, &gvk()).await {
        Ok((resource, _)) => Ok(Some(resource)),
        Err(kube::Error::Api(err)) if err.code == 404 => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn select_port(listener: &Listener, config: &OpenShiftRouteConfig) -> Result<String> {
    let ports = listener.spec.ports.iter().flatten().collect::<Vec<_>>();
    let name = match (&config.port, ports.as_slice()) {
        (Some(name), _) => name,
        (None, [port]) => &port.name,
        (None, ports) => return AmbiguousPortSnafu { count: ports.len() }.fail(),
    };
    ports
        .iter()
        .find(|port| &port.name == name && port.protocol.as_deref().unwrap_or("TCP") == "TCP")
        .map(|port| port.name.clone())
        .context(NoTcpPortSnafu { port: name })
}

pub fn external_port(tls: OpenShiftRouteTls) -> i32 {
    match tls {
        OpenShiftRouteTls::Passthrough => 443,
        OpenShiftRouteTls::None => 80,
    }
}

pub fn build(
    resource: &ApiResource,
    service: &Service,
    port: &str,
    tls: OpenShiftRouteTls,
) -> DynamicObject {
    let mut spec = json!({
        "to": {"kind": "Service", "name": service.name_any()},
        "port": {"targetPort": port},
    });
    if tls == OpenShiftRouteTls::Passthrough {
        spec["tls"] =
            json!({"termination": "passthrough", "insecureEdgeTerminationPolicy": "None"});
    }
    let mut route = DynamicObject::new(&service.name_any(), resource).data(json!({"spec": spec}));
    route.metadata.namespace = service.metadata.namespace.clone();
    route.metadata.labels = service.metadata.labels.clone();
    route.metadata.owner_references = service.metadata.owner_references.clone();
    route
}

pub async fn apply(
    client: kube::Client,
    resource: &ApiResource,
    field_manager: &str,
    route: &DynamicObject,
    listener_uid: &str,
) -> Result<DynamicObject> {
    let name = route.name_any();
    let namespace = route.namespace().context(NoNamespaceSnafu)?;
    let api = Api::<DynamicObject>::namespaced_with(client, &namespace, resource);
    if let Some(existing) = api
        .get_opt(&name)
        .await
        .context(GetRouteSnafu { name: &name })?
    {
        ensure!(
            is_owned_by_listener(existing.owner_references(), listener_uid),
            RefuseToOverwriteForeignRouteSnafu { name }
        );
    }
    api.patch(
        &name,
        &PatchParams::apply(field_manager).force(),
        &Patch::Apply(route),
    )
    .await
    .context(ApplyRouteSnafu { name })
}

pub async fn delete(
    client: kube::Client,
    resource: &ApiResource,
    namespace: &str,
    name: &str,
    listener_uid: &str,
) -> Result<()> {
    let api = Api::<DynamicObject>::namespaced_with(client, namespace, resource);
    if let Some(existing) = api.get_opt(name).await.context(GetRouteSnafu { name })?
        && is_owned_by_listener(existing.owner_references(), listener_uid)
    {
        let params = DeleteParams {
            preconditions: Some(Preconditions {
                uid: existing.metadata.uid,
                resource_version: None,
            }),
            ..DeleteParams::default()
        };
        api.delete(name, &params)
            .await
            .context(DeleteRouteSnafu { name })?;
    }
    Ok(())
}

pub fn admitted_hosts(route: &DynamicObject) -> Vec<String> {
    let ingresses = route.data["status"]["ingress"]
        .as_array()
        .into_iter()
        .flatten();
    ingresses
        .filter(|ingress| {
            ingress["conditions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|cond| cond["type"] == "Admitted" && cond["status"] == "True")
        })
        .filter_map(|ingress| ingress["host"].as_str())
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stackable_operator::{
        crd::listener::v1alpha1::{Listener, OpenShiftRouteConfig},
        kube::api::{ApiResource, DynamicObject},
    };

    use super::{admitted_hosts, gvk, select_port};

    fn listener(ports: serde_json::Value) -> Listener {
        serde_json::from_value(json!({
            "apiVersion": "listeners.stackable.tech/v1alpha1",
            "kind": "Listener",
            "metadata": {"name": "listener"},
            "spec": {"ports": ports},
        }))
        .unwrap()
    }

    fn config(port: Option<&str>) -> OpenShiftRouteConfig {
        OpenShiftRouteConfig {
            port: port.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn select_port_defaults_to_only_port() {
        let listener = listener(json!([{"name": "https", "port": 8443}]));
        assert_eq!(select_port(&listener, &config(None)).unwrap(), "https");
    }

    #[test]
    fn select_port_requires_explicit_port_if_ambiguous() {
        let listener = listener(json!([
            {"name": "https", "port": 8443},
            {"name": "metrics", "port": 9090},
        ]));
        assert!(select_port(&listener, &config(None)).is_err());
        assert_eq!(
            select_port(&listener, &config(Some("https"))).unwrap(),
            "https"
        );
        assert!(select_port(&listener, &config(Some("http"))).is_err());
    }

    #[test]
    fn select_port_rejects_udp() {
        let listener = listener(json!([{"name": "dns", "port": 53, "protocol": "UDP"}]));
        assert!(select_port(&listener, &config(None)).is_err());
    }

    #[test]
    fn admitted_hosts_ignores_unadmitted_ingresses() {
        let route = DynamicObject::new("listener", &ApiResource::from_gvk(&gvk())).data(json!({
            "status": {"ingress": [
                {"host": "admitted.example.com", "conditions": [{"type": "Admitted", "status": "True"}]},
                {"host": "rejected.example.com", "conditions": [{"type": "Admitted", "status": "False"}]},
                {"host": "pending.example.com"},
            ]},
        }));
        assert_eq!(admitted_hosts(&route), ["admitted.example.com"]);
    }
}
