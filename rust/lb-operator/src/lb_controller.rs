use crate::crd::{
    LoadBalancer, LoadBalancerClass, LoadBalancerIngress, LoadBalancerPort, LoadBalancerSpec,
    LoadBalancerStatus, ServiceType,
};
use futures::StreamExt;
use snafu::Snafu;
use stackable_operator::{
    builder::OwnerReferenceBuilder,
    k8s_openapi::api::core::v1::{
        Endpoints, PersistentVolume, Pod, Service, ServicePort, ServiceSpec,
    },
    kube::{
        api::{ListParams, ObjectMeta},
        runtime::{controller, reflector::ObjectRef},
    },
    logging::controller::{report_controller_reconciled, ReconcilerError},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const FIELD_MANAGER_SCOPE: &str = "loadbalancer";

pub async fn run(client: stackable_operator::client::Client) {
    let controller =
        controller::Controller::new(client.get_all_api::<LoadBalancer>(), ListParams::default());
    let lb_store = controller.store();
    controller
        .owns(client.get_all_api::<Service>(), ListParams::default())
        .watches(
            client.get_all_api::<Endpoints>(),
            ListParams::default(),
            move |endpoints| {
                lb_store
                    .state()
                    .into_iter()
                    .filter(move |lb| {
                        lb.status
                            .as_ref()
                            .and_then(|lbs| lbs.service_name.as_deref())
                            == endpoints.metadata.name.as_deref()
                    })
                    .map(|lb| ObjectRef::from_obj(&*lb))
            },
        )
        .shutdown_on_signal()
        .run(
            reconcile,
            error_policy,
            controller::Context::new(Ctx {
                client: client.clone(),
            }),
        )
        .map(|res| {
            report_controller_reconciled(&client, "loadbalancers.lb.stackable.tech", &res);
        })
        .collect::<()>()
        .await;
}

pub struct Ctx {
    pub client: stackable_operator::client::Client,
}

#[derive(Debug, Snafu)]
pub enum Error {}
impl ReconcilerError for Error {
    fn category(&self) -> &'static str {
        match *self {}
    }

    fn secondary_object(
        &self,
    ) -> Option<
        stackable_operator::kube::runtime::reflector::ObjectRef<
            stackable_operator::kube::api::DynamicObject,
        >,
    > {
        match *self {}
    }
}

pub async fn reconcile(
    lb: Arc<LoadBalancer>,
    ctx: controller::Context<Ctx>,
) -> Result<controller::Action, Error> {
    let ctx = ctx.get_ref();
    let ns = lb.metadata.namespace.clone().unwrap();
    let lb_class = ctx
        .client
        .get::<LoadBalancerClass>(lb.spec.class_name.as_deref().unwrap(), None)
        .await
        .unwrap();
    let svc = Service {
        metadata: ObjectMeta {
            namespace: Some(ns.clone()),
            name: lb.metadata.name.clone(),
            owner_references: Some(vec![OwnerReferenceBuilder::new()
                .initialize_from_resource(&*lb)
                .build()
                .unwrap()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some(match lb_class.spec.service_type {
                ServiceType::NodePort => "NodePort".to_string(),
                ServiceType::LoadBalancer => "LoadBalancer".to_string(),
            }),
            ports: Some(
                lb.spec
                    .ports
                    .iter()
                    .flatten()
                    .map(
                        |LoadBalancerPort {
                             name,
                             port,
                             protocol,
                         }| ServicePort {
                            name: Some(name.clone()),
                            protocol: protocol.clone(),
                            port: *port,
                            ..Default::default()
                        },
                    )
                    .collect(),
            ),
            external_traffic_policy: Some("Local".to_string()),
            selector: lb.spec.pod_selector.clone(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let svc = ctx
        .client
        .apply_patch(FIELD_MANAGER_SCOPE, &svc, &svc)
        .await
        .unwrap();

    let addresses: Vec<_>;
    let ports: BTreeMap<String, i32>;
    match lb_class.spec.service_type {
        ServiceType::NodePort => {
            let endpoints = ctx
                .client
                .get::<Endpoints>(svc.metadata.name.as_deref().unwrap(), Some(&ns))
                .await
                .unwrap();
            addresses = endpoints
                .subsets
                .into_iter()
                .flatten()
                .flat_map(|subset| subset.addresses)
                .flatten()
                .flat_map(|addr| addr.node_name)
                .collect();
            ports = svc
                .spec
                .as_ref()
                .unwrap()
                .ports
                .as_ref()
                .unwrap()
                .iter()
                .map(|port| (port.name.clone().unwrap(), port.node_port.unwrap()))
                .collect();
        }
        ServiceType::LoadBalancer => {
            addresses = svc
                .status
                .iter()
                .flat_map(|ss| ss.load_balancer.as_ref()?.ingress.as_ref())
                .flatten()
                .flat_map(|ingress| ingress.hostname.clone().or_else(|| ingress.ip.clone()))
                .collect();
            ports = svc
                .spec
                .as_ref()
                .unwrap()
                .ports
                .as_ref()
                .unwrap()
                .iter()
                .map(|port| (port.name.clone().unwrap(), port.port))
                .collect();
        }
    };

    let lb_status_meta = LoadBalancer {
        metadata: ObjectMeta {
            name: lb.metadata.name.clone(),
            namespace: lb.metadata.namespace.clone(),
            uid: lb.metadata.uid.clone(),
            ..Default::default()
        },
        spec: LoadBalancerSpec::default(),
        status: None,
    };
    let lb_status = LoadBalancerStatus {
        service_name: svc.metadata.name,
        ingress_addresses: Some(
            addresses
                .into_iter()
                .map(|addr| LoadBalancerIngress {
                    address: addr,
                    ports: ports.clone(),
                })
                .collect(),
        ),
    };
    ctx.client
        .apply_patch_status(FIELD_MANAGER_SCOPE, &lb_status_meta, &lb_status)
        .await
        .unwrap();

    Ok(controller::Action::await_change())
}

pub fn error_policy(err: &Error, ctx: controller::Context<Ctx>) -> controller::Action {
    controller::Action::requeue(Duration::from_secs(5))
}
