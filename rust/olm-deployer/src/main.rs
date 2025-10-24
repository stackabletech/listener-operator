/// This program acts as a proxy Deployment in OLM environments that installs the listener operator.
/// The operator manifests are read from a directory and patched before being submitted to the
/// control plane.
/// It expects the following objects to exist (they are created by OLM) and uses them as
/// sources for patch data:
/// - A Deployment owned by the CSV in the target namespace.
/// - A ClusterRole owned by the same CSV that deployed this tool.
///
/// See the documentation of the `maybe_*` functions for patching details.
///
/// The `keep-alive` cli option prevents the program from finishing and thus for OLM
/// to observe it as a failure.
///
mod data;
mod env;
mod owner;
mod resources;
mod tolerations;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use stackable_operator::{
    cli::Command,
    client,
    commons::networking::DomainName,
    k8s_openapi::api::{apps::v1::Deployment, rbac::v1::ClusterRole},
    kube::{
        self,
        api::{Api, DynamicObject, ListParams, Patch, PatchParams, ResourceExt},
        core::GroupVersionKind,
        discovery::{ApiResource, Discovery, Scope},
    },
    telemetry::{Tracing, tracing::TelemetryOptions},
    utils::cluster_info::KubernetesClusterInfoOptions,
};

pub const APP_NAME: &str = "stkbl-listener-olm-deployer";

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(clap::Parser)]
#[clap(author, version)]
struct Opts {
    #[clap(subcommand)]
    cmd: Command<OlmDeployerRun>,
}

#[derive(clap::Parser)]
struct OlmDeployerRun {
    #[arg(
        long,
        default_value = "false",
        help = "Keep running after manifests have been successfully applied."
    )]
    keep_alive: bool,

    #[arg(
        long,
        help = "Name of ClusterServiceVersion object that owns this Deployment."
    )]
    csv: String,

    #[arg(long, help = "Name of deployment object that owns this Pod.")]
    deployer: String,

    #[arg(long, help = "Namespace of the ClusterServiceVersion object.")]
    namespace: String,

    #[arg(long, help = "Directory with manifests to patch and apply.")]
    dir: std::path::PathBuf,

    #[command(flatten)]
    pub telemetry: TelemetryOptions,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse();
    if let Command::Run(OlmDeployerRun {
        keep_alive,
        csv,
        deployer,
        namespace,
        dir,
        telemetry,
    }) = opts.cmd
    {
        // NOTE (@NickLarsenNZ): Before stackable-telemetry was used:
        // - The console log level was set by `STKBL_LISTENER_OLM_DEPLOYER_LOG`, and is now `CONSOLE_LOG` (when using Tracing::pre_configured).
        // - The file log level was (maybe?) set by `STKBL_LISTENER_OLM_DEPLOYER_LOG`, and is now set via `FILE_LOG` (when using Tracing::pre_configured).
        // - The file log directory was set by `STKBL_LISTENER_OLM_DEPLOYER_LOG_DIRECTORY`, and is now set by `ROLLING_LOGS_DIR` (or via `--rolling-logs <DIRECTORY>`).
        let _tracing_guard = Tracing::pre_configured(built_info::PKG_NAME, telemetry).init()?;

        tracing::info!(
            built_info.pkg_version = built_info::PKG_VERSION,
            built_info.git_version = built_info::GIT_VERSION,
            built_info.target = built_info::TARGET,
            built_info.built_time_utc = built_info::BUILT_TIME_UTC,
            built_info.rustc_version = built_info::RUSTC_VERSION,
            "Starting {description}",
            description = built_info::PKG_DESCRIPTION
        );

        // Not used by the olm deployer but still want to use client::initialize_operator()
        // Without this dummy value, the KUBERNETES_NODE_NAME env/cli argument would be required
        // but not used.
        let dummy_cluster_info = KubernetesClusterInfoOptions {
            kubernetes_cluster_domain: Some(DomainName::try_from("cluster.local")?),
            kubernetes_node_name: "".to_string(),
        };

        let client =
            client::initialize_operator(Some(APP_NAME.to_string()), &dummy_cluster_info).await?;

        let deployment = get_deployment(&csv, &deployer, &namespace, &client).await?;
        let cluster_role = get_cluster_role(&csv, &client).await?;

        let kube_client = client.as_kube_client();
        // discovery (to be able to infer apis from kind/plural only)
        let discovery = Discovery::new(kube_client.clone()).run().await?;

        for entry in walkdir::WalkDir::new(&dir) {
            match entry {
                Ok(manifest_file) => {
                    if manifest_file.file_type().is_file() {
                        // ----------
                        let path = manifest_file.path();
                        tracing::info!("Reading manifest file: {}", path.display());
                        let yaml = std::fs::read_to_string(path)
                            .with_context(|| format!("Failed to read {}", path.display()))?;
                        for doc in multidoc_deserialize(&yaml)? {
                            let mut obj: DynamicObject = serde_yaml::from_value(doc)?;
                            // ----------
                            let gvk = if let Some(tm) = &obj.types {
                                GroupVersionKind::try_from(tm)?
                            } else {
                                bail!("cannot apply object without valid TypeMeta {:?}", obj);
                            };
                            let (ar, caps) = discovery
                                .resolve_gvk(&gvk)
                                .context(anyhow!("cannot resolve GVK {:?}", gvk))?;

                            let api = dynamic_api(ar, &caps.scope, kube_client.clone(), &namespace);
                            // ---------- patch object
                            tolerations::maybe_copy_tolerations(&deployment, &mut obj, &gvk)?;
                            owner::maybe_update_owner(
                                &mut obj,
                                &caps.scope,
                                &deployment,
                                &cluster_role,
                            )?;
                            env::maybe_copy_env(&deployment, &mut obj, &gvk)?;
                            resources::maybe_copy_resources(&deployment, &mut obj, &gvk)?;
                            // ---------- apply
                            apply(&api, obj, &gvk.kind).await?
                        }
                    }
                }
                Err(e) => {
                    bail!("Error reading manifest file: {}", e);
                }
            }
        }

        if keep_alive {
            // keep the pod running
            tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
        }
    }

    Ok(())
}

async fn apply(api: &Api<DynamicObject>, obj: DynamicObject, kind: &str) -> Result<()> {
    let name = obj.name_any();
    let ssapply = PatchParams::apply(APP_NAME).force();
    tracing::trace!("Applying {}: \n{}", kind, serde_yaml::to_string(&obj)?);
    let data: serde_json::Value = serde_json::to_value(&obj)?;
    let _r = api.patch(&name, &ssapply, &Patch::Apply(data)).await?;
    tracing::info!("applied {} {}", kind, name);
    Ok(())
}

fn multidoc_deserialize(data: &str) -> Result<Vec<serde_yaml::Value>> {
    use serde::Deserialize;
    let mut docs = vec![];
    for de in serde_yaml::Deserializer::from_str(data) {
        docs.push(serde_yaml::Value::deserialize(de)?);
    }
    Ok(docs)
}

fn dynamic_api(
    ar: ApiResource,
    scope: &Scope,
    client: kube::Client,
    ns: &str,
) -> Api<DynamicObject> {
    match scope {
        Scope::Cluster => Api::all_with(client, &ar),
        _ => Api::namespaced_with(client, ns, &ar),
    }
}

async fn get_cluster_role(csv: &str, client: &client::Client) -> Result<ClusterRole> {
    let labels = format!("olm.owner={csv},olm.owner.kind=ClusterServiceVersion");
    let lp = ListParams {
        label_selector: Some(labels.clone()),
        ..ListParams::default()
    };

    let cluster_role_api = client.get_all_api::<ClusterRole>();
    let result = cluster_role_api.list(&lp).await?.items;
    if !result.is_empty() {
        Ok(result
            .first()
            .context(anyhow!("ClusterRole object not found for labels {labels}"))?
            .clone())
    } else {
        bail!("ClusterRole object not found for labels {labels}")
    }
}

async fn get_deployment(
    csv: &str,
    deployer: &str,
    namespace: &str,
    client: &client::Client,
) -> Result<Deployment> {
    let labels = format!("olm.owner={csv},olm.owner.kind=ClusterServiceVersion");
    let lp = ListParams {
        label_selector: Some(labels.clone()),
        ..ListParams::default()
    };

    let deployment_api = client.get_api::<Deployment>(namespace);
    let result = deployment_api.list(&lp).await?.items;

    match result.len() {
        0 => bail!("no deployment owned by the csv {csv} found in namespace {namespace}"),
        _ => Ok(result
            .into_iter()
            .find(|d| d.name_any() == deployer)
            .context(format!("no deployment named {deployer} found"))?),
    }
}
