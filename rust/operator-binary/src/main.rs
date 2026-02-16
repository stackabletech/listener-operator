// TODO: Look into how to properly resolve `clippy::result_large_err`.
// This will need changes in our and upstream error types.
#![allow(clippy::result_large_err)]
use std::{os::unix::prelude::FileTypeExt, path::PathBuf};

use anyhow::anyhow;
use clap::Parser;
use csi_grpc::v1::{
    controller_server::ControllerServer, identity_server::IdentityServer, node_server::NodeServer,
};
use csi_server::{
    controller::ListenerOperatorController, identity::ListenerOperatorIdentity,
    node::ListenerOperatorNode,
};
use futures::{FutureExt, TryFutureExt, TryStreamExt};
use stackable_operator::{
    self, YamlSchema,
    cli::{Command, CommonOptions, MaintenanceOptions, OperatorEnvironmentOptions},
    client::Client,
    crd::listener::{
        Listener, ListenerClass, ListenerClassVersion, ListenerVersion, PodListeners,
        PodListenersVersion, v1alpha1,
    },
    eos::EndOfSupportChecker,
    shared::yaml::SerializeOptions,
    telemetry::Tracing,
    utils::signal::SignalWatcher,
};
use tokio::sync::oneshot;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use utils::unix_stream::{TonicUnixStream, uds_bind_private};

use crate::webhooks::conversion::create_webhook_server;

mod csi_server;
mod listener_controller;
mod utils;
mod webhooks;

const APP_NAME: &str = "listener";
const OPERATOR_KEY: &str = "listeners.stackable.tech";
const FIELD_MANAGER: &str = "listener-operator";

#[derive(clap::Parser)]
#[clap(author, version)]
struct Cli {
    #[clap(subcommand)]
    cmd: Command<ListenerOperatorRun>,
}

#[derive(clap::Parser)]
struct ListenerOperatorRun {
    #[arg(long, env)]
    csi_endpoint: PathBuf,

    #[clap(subcommand)]
    mode: RunMode,

    // IMPORTANT: All (flattened) sub structs should be placed at the end to ensure the help
    // headings are correct.
    #[command(flatten)]
    common: CommonOptions,

    #[command(flatten)]
    maintenance: MaintenanceOptions,

    #[command(flatten)]
    operator_environment: OperatorEnvironmentOptions,
}

#[derive(Debug, clap::Parser, strum::AsRefStr, strum::Display)]
enum RunMode {
    /// CSI Controller Service
    Controller(ControllerArguments),

    /// CSI Node Service
    Node,
}

#[derive(Debug, clap::Args)]
struct ControllerArguments {
    #[arg(long, env, default_value_t)]
    listener_class_preset: ListenerClassPreset,
}

#[derive(Clone, Debug, Default, clap::Parser, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
enum ListenerClassPreset {
    /// Deploys no listener class preset.
    None,

    /// Deploys listener classes for environments in which pods can move freely
    /// between nodes. This is common for many managed cloud environments.
    #[default]
    EphemeralNodes,

    /// Deploys listener classes for environments with reliable, long-living
    /// nodes and pods don't move between nodes.
    StableNodes,
}

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

// TODO (@NickLarsenNZ): Change the variable to `CONSOLE_LOG`
pub const ENV_VAR_CONSOLE_LOG: &str = "LISTENER_OPERATOR_LOG";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Cli::parse();
    match opts.cmd {
        Command::Crd => {
            ListenerClass::merged_crd(ListenerClassVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
            Listener::merged_crd(ListenerVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
            PodListeners::merged_crd(PodListenersVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
        }
        Command::Run(ListenerOperatorRun {
            operator_environment,
            csi_endpoint,
            maintenance,
            common,
            mode,
        }) => {
            // NOTE (@NickLarsenNZ): Before stackable-telemetry was used:
            // - The console log level was set by `LISTENER_OPERATOR_LOG`, and is now `CONSOLE_LOG` (when using Tracing::pre_configured).
            // - The file log level was (maybe?) set by `LISTENER_OPERATOR_LOG`, and is now set via `FILE_LOG` (when using Tracing::pre_configured).
            // - The file log directory was set by `LISTENER_OPERATOR_LOG_DIRECTORY`, and is now set by `ROLLING_LOGS_DIR` (or via `--rolling-logs <DIRECTORY>`).
            let _tracing_guard =
                Tracing::pre_configured(built_info::PKG_NAME, common.telemetry).init()?;

            tracing::info!(
                run_mode = %mode,
                built_info.pkg_version = built_info::PKG_VERSION,
                built_info.git_version = built_info::GIT_VERSION,
                built_info.target = built_info::TARGET,
                built_info.built_time_utc = built_info::BUILT_TIME_UTC,
                built_info.rustc_version = built_info::RUSTC_VERSION,
                "Starting {description}",
                description = built_info::PKG_DESCRIPTION
            );

            // Watches for the SIGTERM signal and sends a signal to all receivers, which gracefully
            // shuts down all concurrent tasks below (EoS checker, controller).
            let sigterm_watcher = SignalWatcher::sigterm()?;

            let eos_checker =
                EndOfSupportChecker::new(built_info::BUILT_TIME_UTC, maintenance.end_of_support)?
                    .run(sigterm_watcher.handle())
                    .map(anyhow::Ok);

            let client = stackable_operator::client::initialize_operator(
                Some(OPERATOR_KEY.to_string()),
                &common.cluster_info,
            )
            .await?;

            if csi_endpoint
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_socket())
            {
                let _ = std::fs::remove_file(&csi_endpoint);
            }

            let csi_listener =
                UnixListenerStream::new(uds_bind_private(csi_endpoint)?).map_ok(TonicUnixStream);

            let csi_server = Server::builder()
                .add_service(
                    tonic_reflection::server::Builder::configure()
                        .include_reflection_service(true)
                        .register_encoded_file_descriptor_set(csi_grpc::FILE_DESCRIPTOR_SET_BYTES)
                        .build_v1()?,
                )
                .add_service(IdentityServer::new(ListenerOperatorIdentity));

            match mode {
                RunMode::Controller(ControllerArguments {
                    listener_class_preset,
                }) => {
                    let (webhook_server, initial_reconcile_rx) = create_webhook_server(
                        &operator_environment,
                        maintenance.disable_crd_maintenance,
                        client.as_kube_client(),
                    )
                    .await?;

                    let webhook_server = webhook_server
                        .run(sigterm_watcher.handle())
                        .map_err(|err| anyhow!(err).context("failed to run webhook server"));

                    let listener_classes = create_listener_classes(
                        initial_reconcile_rx,
                        listener_class_preset,
                        client.clone(),
                    )
                    .map_err(|err| {
                        anyhow!(err).context("failed to apply listener classes selected by preset")
                    });

                    let csi_server = csi_server
                        .add_service(ControllerServer::new(ListenerOperatorController {
                            client: client.clone(),
                        }))
                        .serve_with_incoming_shutdown(csi_listener, sigterm_watcher.handle())
                        .map_err(|err| anyhow!(err).context("failed to run csi server"));

                    let controller =
                        listener_controller::run(client, sigterm_watcher.handle()).map(anyhow::Ok);

                    futures::try_join!(
                        listener_classes,
                        webhook_server,
                        eos_checker,
                        csi_server,
                        controller,
                    )?;
                }
                RunMode::Node => {
                    let node_name = &common.cluster_info.kubernetes_node_name;
                    let csi_server = csi_server
                        .add_service(NodeServer::new(ListenerOperatorNode {
                            client: client.clone(),
                            node_name: node_name.to_owned(),
                        }))
                        .serve_with_incoming_shutdown(csi_listener, sigterm_watcher.handle())
                        .map_err(|err| anyhow!(err).context("failed to run csi server"));

                    futures::try_join!(csi_server, eos_checker)?;
                }
            }
        }
    }

    Ok(())
}

async fn create_listener_classes(
    initial_reconcile_rx: oneshot::Receiver<()>,
    listener_class_preset: ListenerClassPreset,
    client: Client,
) -> anyhow::Result<()> {
    initial_reconcile_rx.await?;

    tracing::info!("applying \"{listener_class_preset}\" listener class preset");

    #[rustfmt::skip]
    let bytes = match listener_class_preset {
        ListenerClassPreset::None => return Ok(()),
        ListenerClassPreset::EphemeralNodes => include_bytes!("manifests/ephemeral-nodes.yaml").to_vec(),
        ListenerClassPreset::StableNodes => include_bytes!("manifests/stable-nodes.yaml").to_vec(),
    };

    for document in serde_yaml::Deserializer::from_slice(&bytes) {
        let class: v1alpha1::ListenerClass =
            serde_yaml::with::singleton_map_recursive::deserialize(document)
                .expect("compile-time included listener classes must be valid YAML");

        client.create_if_missing(&class).await?;
    }

    Ok(())
}
