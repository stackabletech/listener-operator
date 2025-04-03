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
    CustomResourceExt,
    commons::listener::{Listener, ListenerClass, PodListeners},
    logging::TracingTarget,
    utils::cluster_info::KubernetesClusterInfoOpts,
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
    #[arg(long, env, default_value_t, value_enum)]
    tracing_target: TracingTarget,

    #[clap(long, env)]
    csi_endpoint: PathBuf,

    #[clap(subcommand)]
    mode: RunMode,

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
            tracing_target,
            csi_endpoint,
            mode,
            cluster_info_opts,
        }) => {
            stackable_operator::logging::initialize_logging(
                "LISTENER_OPERATOR_LOG",
                "listener-operator",
                tracing_target,
            );
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
