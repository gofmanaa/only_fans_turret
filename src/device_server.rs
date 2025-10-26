mod action_service;
mod actions;
mod devices;
mod gst_v8_stream;

use crate::action_service::ActionService;
use crate::devices::grpc_server::GrpcDeviceServer;
#[cfg(feature = "gstream")]
use crate::gst_v8_stream::gstream::video_stream_start;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::signal;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser)]
struct Cli {
    #[clap(short, long, default_value = "127.0.0.1:5001", env = "GRPC_ADDR")]
    grpc_addr: SocketAddr,

    #[clap(short = 't', long, default_value = "/dev/ttyUSB0", env = "STTY_PATH")]
    stty_path: PathBuf,

    #[clap(short, long, default_value = "9600", env = "BAUD_RATE")]
    baud_rate: u32,

    #[clap(short, long, default_value = "/dev/video0", env = "VIDEO_DEV")]
    video_dev: PathBuf,

    #[clap(long, default_value = "127.0.0.1:5004", env = "V8STREAM_URL")]
    v8stream_url: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

    let action_service = ActionService::new(cli.stty_path.as_path(), cli.baud_rate).await?;

    let device_server = GrpcDeviceServer::new(action_service);

    // Spawn gRPC server
    let grpc_handle = tokio::spawn(async move {
        info!("gRPC server listening on {}", cli.grpc_addr);
        Server::builder()
            .add_service(device_server.into_service())
            .serve(cli.grpc_addr)
            .await
            .expect("Grpc server failed to start");
    });

    #[cfg(feature = "gstream")]
    info!("GStream enabled!");

    #[cfg(feature = "gstream")]
    let video_handle = video_stream_start(cli.video_dev, cli.v8stream_url);

    // Wait for Ctrl+C
    signal::ctrl_c().await?;
    info!("Ctrl+C received, stopping...");

    #[cfg(feature = "gstream")]
    video_handle.thread().unpark();

    grpc_handle.abort();

    Ok(())
}


