use std::{os::unix::prelude::FileTypeExt, path::PathBuf};

use clap::Parser;
use csi_server::{
    controller::LbOperatorController, identity::LbOperatorIdentity, node::LbOperatorNode,
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

use crate::crd::{LoadBalancer, LoadBalancerClass};

mod crd;
mod csi_server;
mod grpc;
mod lb_controller;
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
        stackable_operator::cli::Command::Crd => {
            println!(
                "{}{}",
                serde_yaml::to_string(&LoadBalancerClass::crd()).unwrap(),
                serde_yaml::to_string(&LoadBalancer::crd()).unwrap()
            );
        }
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
            let csi_server = Server::builder()
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
                .add_service(NodeServer::new(LbOperatorNode {
                    client: client.clone(),
                    node_name,
                }))
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(uds_bind_private(csi_endpoint)?)
                        .map_ok(TonicUnixStream),
                    sigterm.recv().map(|_| ()),
                );
            let controller = lb_controller::run(client);
            pin_mut!(csi_server, controller);
            futures::future::select(csi_server, controller).await;
        }
    }
    Ok(())
}
