mod config;
mod handler;
mod message;
mod rtp;
mod sdp_handler;
mod turn;

mod app_state;

use crate::app_state::AppState;
use crate::config::WebConfig;
use crate::handler::{serve_index, websocket_handler};
use crate::rtp::rtp_thread;
use crate::sdp_handler::{get_turn_credentials, handle_sdp_offer};
use anyhow::anyhow;
use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use device::pb::device_client::DeviceClient;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tonic::transport::{Channel, Endpoint};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice::network_type::NetworkType;
use webrtc::interceptor::registry::Registry;

#[derive(Parser)]
struct Cli {
    #[clap(short, long, default_value = "0.0.0.0:8080", env = "SERVER_ADDR")]
    servet_addr: SocketAddr,

    #[clap(
        short,
        long,
        default_value = "grpc://127.0.0.1:5001",
        env = "DEVICE_SERVER_ADDR"
    )]
    device_server: String,

    #[clap(short, long, default_value = "0.0.0.0:5004", env = "RTP_ADDR")]
    rtp_addr: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // let (layer, task) = tracing_loki::builder()
    //     .label("host", "mine")?
    //     .extra_field("pid", format!("{}", process::id()))?
    //     .http_header("X-Scope-OrgID", "tenant1")?
    //     .build_url(Url::parse("http://127.0.0.1:3100").unwrap())?;
    tracing_subscriber::registry()
        //  .with(layer)
        .with(tracing_subscriber::fmt::Layer::new())
        .with(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();

    // tracing_subscriber::fmt::init();
    // tokio::spawn(task);

    let cli = Cli::parse();
    let web_config = WebConfig::new()?;

    // -------------------------
    // WebRTC API setup
    // -------------------------
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let mut setting_engine = SettingEngine::default();
    setting_engine.set_network_types(vec![NetworkType::Udp4]); // Needed for IPv4

    let api = Arc::new(
        APIBuilder::new()
            .with_setting_engine(setting_engine)
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build(),
    );

    let device_gpc_client = connect_device_server(&cli.device_server).await?;

    let state = Arc::new(AppState::new(api, device_gpc_client, web_config));

    state.start_background_tasks();

    rtp_thread(cli.rtp_addr, state.clone());

    let web_dir = std::env::current_dir()?.join("web");

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .route("/sdp", post(handle_sdp_offer))
        .route("/turn", post(get_turn_credentials))
        .nest_service("/static", ServeDir::new(web_dir))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cli.servet_addr.to_owned()).await?;

    info!("Server starting on {}", cli.servet_addr);

    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn connect_device_server(
    device_server: &str,
) -> anyhow::Result<Arc<Mutex<DeviceClient<Channel>>>> {
    const MAX_RETRIES: u32 = 100;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    for attempt in 1..=MAX_RETRIES {
        let endpoint = Endpoint::from_shared(device_server.to_string())?;

        match endpoint.connect().await {
            Ok(channel) => {
                info!("Connected to device server at {}", device_server);
                let client = DeviceClient::new(channel);
                return Ok(Arc::new(Mutex::new(client)));
            }
            Err(e) => {
                error!(
                    "Failed to connect to device server {} (attempt {}/{}): {}",
                    device_server, attempt, MAX_RETRIES, e
                );

                if attempt == MAX_RETRIES {
                    return Err(anyhow!(
                        "Failed to connect to device server {} after {} attempts",
                        device_server,
                        MAX_RETRIES
                    ));
                }

                sleep(RETRY_DELAY).await;
            }
        }
    }
    unreachable!();
}
