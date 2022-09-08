use std::{os::unix::prelude::FileTypeExt, path::PathBuf};

use clap::{crate_description, crate_version, Parser};
use csi_server::{
    controller::ListenerOperatorController, identity::ListenerOperatorIdentity,
    node::ListenerOperatorNode,
};
use futures::{pin_mut, FutureExt, TryStreamExt};
use grpc::csi::v1::{
    controller_server::ControllerServer, identity_server::IdentityServer, node_server::NodeServer,
};
use stackable_operator::{kube::CustomResourceExt, logging::TracingTarget};
use tokio::signal::unix::{signal, SignalKind};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use utils::{uds_bind_private, TonicUnixStream};

use crate::crd::{Listener, ListenerClass};

mod crd;
mod csi_server;
mod grpc;
mod listener_controller;
mod utils;

const OPERATOR_KEY: &str = "listeners.stackable.tech";

#[derive(clap::Parser)]
#[clap(author, version)]
struct Opts {
    #[clap(subcommand)]
    cmd: stackable_operator::cli::Command<LbOperatorRun>,
}

#[derive(clap::Parser)]
struct LbOperatorRun {
    #[clap(long, env)]
    csi_endpoint: PathBuf,
    #[clap(long, env)]
    node_name: String,
    #[clap(long, env, default_value_t, arg_enum)]
    tracing_target: TracingTarget,
}

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
    pub const TARGET: Option<&str> = option_env!("TARGET");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.cmd {
        stackable_operator::cli::Command::Crd => {
            println!(
                "{}{}",
                serde_yaml::to_string(&ListenerClass::crd()).unwrap(),
                serde_yaml::to_string(&Listener::crd()).unwrap()
            );
        }
        stackable_operator::cli::Command::Run(LbOperatorRun {
            csi_endpoint,
            node_name,
            tracing_target,
        }) => {
            stackable_operator::logging::initialize_logging(
                "LISTENER_OPERATOR_LOG",
                "listener-operator",
                tracing_target,
            );
            stackable_operator::utils::print_startup_string(
                crate_description!(),
                crate_version!(),
                built_info::GIT_VERSION,
                built_info::TARGET.unwrap_or("unknown target"),
                built_info::BUILT_TIME_UTC,
                built_info::RUSTC_VERSION,
            );
            let client =
                stackable_operator::client::create_client(Some(OPERATOR_KEY.to_string())).await?;
            if csi_endpoint
                .symlink_metadata()
                .map_or(false, |meta| meta.file_type().is_socket())
            {
                let _ = std::fs::remove_file(&csi_endpoint);
            }
            let mut sigterm = signal(SignalKind::terminate())?;
            let csi_server = Server::builder()
                .add_service(
                    tonic_reflection::server::Builder::configure()
                        .include_reflection_service(true)
                        .register_encoded_file_descriptor_set(grpc::FILE_DESCRIPTOR_SET_BYTES)
                        .build()?,
                )
                .add_service(IdentityServer::new(ListenerOperatorIdentity))
                .add_service(ControllerServer::new(ListenerOperatorController {
                    client: client.clone(),
                }))
                .add_service(NodeServer::new(ListenerOperatorNode {
                    client: client.clone(),
                    node_name,
                }))
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(uds_bind_private(csi_endpoint)?)
                        .map_ok(TonicUnixStream),
                    sigterm.recv().map(|_| ()),
                );
            let controller = listener_controller::run(client);
            pin_mut!(csi_server, controller);
            futures::future::select(csi_server, controller).await;
        }
    }
    Ok(())
}
