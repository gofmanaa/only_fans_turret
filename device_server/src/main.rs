#[cfg(feature = "gstream")]
mod gst_v8_stream;
use clap::Parser;
use device::action_service::ActionService;
#[cfg(feature = "gstream")]
use device::action_service::VideoStreamerHandle;
use device::grpc_server::GrpcDeviceServer;
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

    #[clap(long, default_value = "127.0.0.1", env = "V8STREAM_ADDR")]
    v8stream_addr: String,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

    // Optional: initialize video streamer if gstream feature is enabled
    #[cfg(feature = "gstream")]
    {
        info!("GStream feature enabled!");

        let stream_factory = {
            let video_dev = cli.video_dev.clone();
            let v8_addr = cli.v8stream_addr.clone();

            Box::new(move || gst_v8_stream::video_stream_start(video_dev.clone(), &v8_addr))
                as Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>
        };

        let action_service =
            ActionService::new(cli.stty_path.as_path(), cli.baud_rate, Some(stream_factory))
                .await?;

        let device_server = GrpcDeviceServer::new(action_service);

        let grpc_addr = cli.grpc_addr;

        let grpc_handle = tokio::spawn(async move {
            info!("gRPC server listening on {}", grpc_addr);
            Server::builder()
                .add_service(device_server.into_service())
                .serve(grpc_addr)
                .await
                .expect("Grpc server failed to start");
        });

        signal::ctrl_c().await?;
        info!("Ctrl+C received, stopping...");

        grpc_handle.abort();
        info!("Shutdown complete.");
    }

    #[cfg(not(feature = "gstream"))]
    {
        info!("GStream feature disabled, running in serial-only mode");

        let action_service =
            ActionService::new(cli.stty_path.as_path(), cli.baud_rate, None).await?;

        let device_server = GrpcDeviceServer::new(action_service);

        let grpc_addr = cli.grpc_addr;

        let grpc_handle = tokio::spawn(async move {
            info!("gRPC server listening on {}", grpc_addr);
            Server::builder()
                .add_service(device_server.into_service())
                .serve(grpc_addr)
                .await
                .expect("Grpc server failed to start");
        });

        signal::ctrl_c().await?;
        info!("Ctrl+C received, stopping...");

        grpc_handle.abort();
        info!("Shutdown complete.");
    }

    Ok(())
}
