mod action_service;
mod actions;
mod app_state;
mod devices;
mod handler;
mod message;
mod rtp;
mod sdp_handler;
mod turn;
mod config;

use crate::app_state::AppState;
use crate::devices::pb::device_client::DeviceClient;
use crate::handler::{serve_index, websocket_handler};
use crate::rtp::rtp_thread;
use crate::sdp_handler::{get_turn_credentials, handle_sdp_offer};
use axum::Router;
use axum::routing::{get, post};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tonic::transport::{Channel, Endpoint, Error};
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
use crate::config::WebConfig;

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

    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            state_clone.process_queue().await;
        }
    });

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
) -> Result<Arc<Mutex<DeviceClient<Channel>>>, Error> {
    let mut retries = 0;

    loop {
        let endpoint = Endpoint::from_shared(device_server.to_string())?;

        match endpoint.connect().await {
            Ok(client) => {
                info!("Connected to device server at {}", &device_server);
                let client = DeviceClient::new(client);
                return Ok(Arc::new(Mutex::new(client)));
            }
            Err(e) => {
                retries += 1;
                error!(
                    "Failed to connect to device server {} (attempt {}): {}",
                    device_server, retries, e
                );

                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
