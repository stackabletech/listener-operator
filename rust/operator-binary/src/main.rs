// TODO: Look into how to properly resolve `clippy::result_large_err`.
// This will need changes in our and upstream error types.
#![allow(clippy::result_large_err)]
use std::{os::unix::prelude::FileTypeExt, path::PathBuf};

use clap::Parser;
use csi_grpc::v1::{
    controller_server::ControllerServer, identity_server::IdentityServer, node_server::NodeServer,
};
use csi_server::{
    controller::ListenerOperatorController, identity::ListenerOperatorIdentity,
    node::ListenerOperatorNode,
};
use futures::{FutureExt, TryStreamExt, pin_mut};
use stackable_operator::{
    self, YamlSchema,
    cli::OperatorEnvironmentOptions,
    crd::listener::{
        Listener, ListenerClass, ListenerClassVersion, ListenerVersion, PodListeners,
        PodListenersVersion,
    },
    shared::yaml::SerializeOptions,
    telemetry::{Tracing, tracing::TelemetryOptions},
    utils::cluster_info::KubernetesClusterInfoOptions,
};
use tokio::signal::unix::{SignalKind, signal};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use utils::unix_stream::{TonicUnixStream, uds_bind_private};

mod csi_server;
mod listener_controller;
mod utils;

const APP_NAME: &str = "listener";
const OPERATOR_KEY: &str = "listeners.stackable.tech";

#[derive(clap::Parser)]
#[clap(author, version)]
struct Opts {
    #[clap(subcommand)]
    cmd: stackable_operator::cli::Command<ListenerOperatorRun>,
}

#[derive(clap::Parser)]
struct ListenerOperatorRun {
    #[clap(long, env)]
    csi_endpoint: PathBuf,

    #[clap(subcommand)]
    mode: RunMode,

    #[command(flatten)]
    operator_environment: OperatorEnvironmentOptions,

    #[command(flatten)]
    telemetry: TelemetryOptions,

    #[command(flatten)]
    cluster_info: KubernetesClusterInfoOptions,
}

#[derive(Debug, clap::Parser, strum::AsRefStr, strum::Display)]
enum RunMode {
    Controller,
    Node,
}

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

// TODO (@NickLarsenNZ): Change the variable to `CONSOLE_LOG`
pub const ENV_VAR_CONSOLE_LOG: &str = "LISTENER_OPERATOR_LOG";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.cmd {
        stackable_operator::cli::Command::Crd => {
            ListenerClass::merged_crd(ListenerClassVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
            Listener::merged_crd(ListenerVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
            PodListeners::merged_crd(PodListenersVersion::V1Alpha1)?
                .print_yaml_schema(built_info::PKG_VERSION, SerializeOptions::default())?;
        }
        stackable_operator::cli::Command::Run(ListenerOperatorRun {
            csi_endpoint,
            mode,
            operator_environment: _,
            telemetry,
            cluster_info,
        }) => {
            // NOTE (@NickLarsenNZ): Before stackable-telemetry was used:
            // - The console log level was set by `LISTENER_OPERATOR_LOG`, and is now `CONSOLE_LOG` (when using Tracing::pre_configured).
            // - The file log level was (maybe?) set by `LISTENER_OPERATOR_LOG`, and is now set via `FILE_LOG` (when using Tracing::pre_configured).
            // - The file log directory was set by `LISTENER_OPERATOR_LOG_DIRECTORY`, and is now set by `ROLLING_LOGS_DIR` (or via `--rolling-logs <DIRECTORY>`).
            let _tracing_guard = Tracing::pre_configured(built_info::PKG_NAME, telemetry).init()?;

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
            let client = stackable_operator::client::initialize_operator(
                Some(OPERATOR_KEY.to_string()),
                &cluster_info,
            )
            .await?;
            if csi_endpoint
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_socket())
            {
                let _ = std::fs::remove_file(&csi_endpoint);
            }
            let mut sigterm = signal(SignalKind::terminate())?;
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
                RunMode::Controller => {
                    let csi_server = csi_server
                        .add_service(ControllerServer::new(ListenerOperatorController {
                            client: client.clone(),
                        }))
                        .serve_with_incoming_shutdown(csi_listener, sigterm.recv().map(|_| ()));
                    let controller = listener_controller::run(client).map(Ok);
                    pin_mut!(csi_server, controller);
                    futures::future::try_select(csi_server, controller)
                        .await
                        .map_err(|err| err.factor_first().0)?;
                }
                RunMode::Node => {
                    let node_name = &cluster_info.kubernetes_node_name;
                    csi_server
                        .add_service(NodeServer::new(ListenerOperatorNode {
                            client: client.clone(),
                            node_name: node_name.to_owned(),
                        }))
                        .serve_with_incoming_shutdown(csi_listener, sigterm.recv().map(|_| ()))
                        .await?;
                }
            }
        }
    }
    Ok(())
}
