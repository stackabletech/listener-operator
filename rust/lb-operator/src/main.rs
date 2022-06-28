use std::{os::unix::prelude::FileTypeExt, path::PathBuf};

use clap::Parser;
use csi_server::{
    controller::LbOperatorController, identity::LbOperatorIdentity, node::LbOperatorNode,
};
use futures::{FutureExt, TryStreamExt};
use grpc::csi::v1::{
    controller_server::ControllerServer, identity_server::IdentityServer, node_server::NodeServer,
};
use stackable_operator::{cli::ProductOperatorRun, logging::TracingTarget};
use tokio::signal::unix::{signal, SignalKind};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use utils::{uds_bind_private, TonicUnixStream};

mod csi_server;
mod grpc;
mod utils;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.cmd {
        stackable_operator::cli::Command::Crd => {}
        stackable_operator::cli::Command::Run(LbOperatorRun {
            csi_endpoint,
            node_name,
            tracing_target,
        }) => {
            stackable_operator::logging::initialize_logging(
                "LB_OPERATOR_LOG",
                "lb-operator",
                tracing_target,
            );
            let client =
                stackable_operator::client::create_client(Some("lb.stackable.tech".to_string()))
                    .await?;
            if csi_endpoint
                .symlink_metadata()
                .map_or(false, |meta| meta.file_type().is_socket())
            {
                let _ = std::fs::remove_file(&csi_endpoint);
            }
            let mut sigterm = signal(SignalKind::terminate())?;
            Server::builder()
                .add_service(
                    tonic_reflection::server::Builder::configure()
                        .include_reflection_service(true)
                        .register_encoded_file_descriptor_set(grpc::FILE_DESCRIPTOR_SET_BYTES)
                        .build()?,
                )
                .add_service(IdentityServer::new(LbOperatorIdentity))
                .add_service(ControllerServer::new(LbOperatorController {
                    client: client.clone(),
                }))
                .add_service(NodeServer::new(LbOperatorNode { client, node_name }))
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(uds_bind_private(csi_endpoint)?)
                        .map_ok(TonicUnixStream),
                    sigterm.recv().map(|_| ()),
                )
                .await?;
        }
    }
    Ok(())
}
