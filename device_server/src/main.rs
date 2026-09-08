#[cfg(feature = "gstream")]
mod gst_v8_stream;

use clap::Parser;
use device::action_service::ActionService;

#[cfg(feature = "turret")]
use device::grpc_server::GrpcDeviceServer;

#[cfg(feature = "gstream")]
use device::action_service::VideoStreamerHandle;

use std::net::SocketAddr;

use std::path::PathBuf;

use tokio::signal;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// Turret and CameraOnly cannot be enabled together.
#[cfg(all(feature = "turret", feature = "cameraonly"))]
compile_error!("Features 'turret' and 'cameraonly' are mutually exclusive");

// One device type must be selected.
#[cfg(not(any(feature = "turret", feature = "cameraonly")))]
compile_error!("Enable either 'turret' or 'cameraonly'");

// CameraOnly always needs GStreamer.
#[cfg(all(feature = "cameraonly", not(feature = "gstream")))]
compile_error!("Feature 'cameraonly' requires 'gstream'");

#[derive(Parser)]
struct Cli {
    #[clap(short, long, default_value = "127.0.0.1:5001", env = "GRPC_ADDR")]
    grpc_addr: SocketAddr,

    #[cfg(feature = "turret")]
    #[clap(short = 't', long, default_value = "/dev/ttyUSB0", env = "STTY_PATH")]
    stty_path: PathBuf,

    #[cfg(feature = "turret")]
    #[clap(short, long, default_value = "9600", env = "BAUD_RATE")]
    baud_rate: u32,

    #[cfg(feature = "gstream")]
    #[clap(short, long, default_value = "/dev/video0", env = "VIDEO_DEV")]
    video_dev: PathBuf,

    #[cfg(feature = "gstream")]
    #[clap(long, default_value = "127.0.0.1:5004", env = "V8STREAM_ADDR")]
    v8stream_addr: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

    #[cfg(feature = "gstream")]
    let stream_factory = {
        info!("GStreamer feature enabled");

        let video_dev = cli.video_dev;
        let stream_addr = cli.v8stream_addr;

        info!(
            "Video device configured: {}, stream to {}",
            video_dev.display(),
            stream_addr
        );

        Box::new(move || gst_v8_stream::video_stream_start(video_dev.clone(), stream_addr))
            as Box<dyn Fn() -> VideoStreamerHandle + Send + Sync>
    };

    #[cfg(not(feature = "gstream"))]
    info!("GStreamer feature disabled");

    #[cfg(feature = "turret")]
    let device_server = {
        info!("Starting Turret device");

        #[cfg(feature = "gstream")]
        let action_service =
            ActionService::new(cli.stty_path.as_path(), cli.baud_rate, Some(stream_factory))
                .await?;

        #[cfg(not(feature = "gstream"))]
        let action_service =
            ActionService::new(cli.stty_path.as_path(), cli.baud_rate, None).await?;

        GrpcDeviceServer::new(action_service).into_service()
    };

    #[cfg(feature = "cameraonly")]
    let device_server = {
        info!("Starting CameraOnly device");

        use device::cameraonly_server::GrpcCameraServer;

        let action_service = ActionService::new_stream(stream_factory);

        GrpcCameraServer::new(action_service).into_service()
    };

    let grpc_addr = cli.grpc_addr;

    info!("gRPC server listening on {}", grpc_addr);

    let shutdown_signal = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Ctrl+C received, stopping...");
            }
            Err(e) => {
                tracing::error!("Failed to listen for Ctrl+C: {}", e);
            }
        }
    };

    Server::builder()
        .add_service(device_server)
        .serve_with_shutdown(grpc_addr, shutdown_signal)
        .await?;

    info!("gRPC server stopped");
    info!("Shutdown complete");

    Ok(())
}
