use std::{ops::Deref as _, os::unix::prelude::FileTypeExt, path::PathBuf};

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
    CustomResourceExt,
    cli::{RollingPeriod, TelemetryArguments},
    commons::listener::{Listener, ListenerClass, PodListeners},
    utils::cluster_info::KubernetesClusterInfoOpts,
};
use stackable_telemetry::{Tracing, tracing::settings::Settings};
use tokio::signal::unix::{SignalKind, signal};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use tracing::level_filters::LevelFilter;
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
    pub telemetry_arguments: TelemetryArguments,

    #[command(flatten)]
    pub cluster_info_opts: KubernetesClusterInfoOpts,
}

#[derive(Debug, clap::Parser, strum::AsRefStr, strum::Display)]
enum RunMode {
    Controller,
    Node {
        #[clap(long, env)]
        node_name: String,
    },
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
            ListenerClass::print_yaml_schema(built_info::PKG_VERSION)?;
            Listener::print_yaml_schema(built_info::PKG_VERSION)?;
            PodListeners::print_yaml_schema(built_info::PKG_VERSION)?;
        }
        stackable_operator::cli::Command::Run(ListenerOperatorRun {
            csi_endpoint,
            mode,
            telemetry_arguments,
            cluster_info_opts,
        }) => {
            let _tracing_guard = Tracing::builder()
                .service_name("listener-operator")
                .with_console_output((
                    ENV_VAR_CONSOLE_LOG,
                    LevelFilter::INFO,
                    !telemetry_arguments.no_console_output,
                ))
                // NOTE (@NickLarsenNZ): Before stackable-telemetry was used, the log directory was
                // set via an env: `LISTENER_OPERATOR_LOG_DIRECTORY`.
                // See: https://github.com/stackabletech/operator-rs/blob/f035997fca85a54238c8de895389cc50b4d421e2/crates/stackable-operator/src/logging/mod.rs#L40
                // Now it will be `ROLLING_LOGS` (or via `--rolling-logs <DIRECTORY>`).
                .with_file_output(telemetry_arguments.rolling_logs.map(|log_directory| {
                    let rotation_period = telemetry_arguments
                        .rolling_logs_period
                        .unwrap_or(RollingPeriod::Hourly)
                        .deref()
                        .clone();

                    Settings::builder()
                        .with_environment_variable(ENV_VAR_CONSOLE_LOG)
                        .with_default_level(LevelFilter::INFO)
                        .file_log_settings_builder(log_directory, "tracing-rs.log")
                        .with_rotation_period(rotation_period)
                        .build()
                }))
                .with_otlp_log_exporter((
                    "OTLP_LOG",
                    LevelFilter::DEBUG,
                    telemetry_arguments.otlp_logs,
                ))
                .with_otlp_trace_exporter((
                    "OTLP_TRACE",
                    LevelFilter::DEBUG,
                    telemetry_arguments.otlp_traces,
                ))
                .build()
                .init()?;

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
                &cluster_info_opts,
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
                RunMode::Node { node_name } => {
                    csi_server
                        .add_service(NodeServer::new(ListenerOperatorNode {
                            client: client.clone(),
                            node_name,
                        }))
                        .serve_with_incoming_shutdown(csi_listener, sigterm.recv().map(|_| ()))
                        .await?;
                }
            }
        }
    }
    Ok(())
}
