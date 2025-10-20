mod action_service;
mod app_state;
mod gst_v8_stream;
mod handler;
mod message;
mod rtp;
mod sdp_handler;

use crate::app_state::AppState;
use crate::handler::websocket_handler;
use crate::rtp::rtp_thread;
use crate::sdp_handler::handle_sdp_offer;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};
// use tracing_loki::url::Url;
use crate::action_service::ActionService;
use clap::Parser;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use uuid::Uuid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::interceptor::registry::Registry;
#[cfg(feature = "gstream")]
use crate::gst_v8_stream::gstream::video_stream_start;

#[derive(Parser)]
struct Cli {
    #[clap(short, long, default_value = "127.0.0.1:8080")]
    servet_addr: SocketAddr,

    #[clap(short, long, default_value = "127.0.0.1:5004")]
    rtp_addr: SocketAddr,

    #[clap(short = 't', long, default_value = "/dev/ttyUSB0")]
    stty_path: PathBuf,

    #[clap(short, long, default_value = "9600")]
    baud_rate: u32,

    #[clap(short, long, default_value = "/dev/video0")]
    video_dev: PathBuf,

    #[clap(long, default_value = "127.0.0.1:5004")]
    v8stream_url: SocketAddr,
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

    // -------------------------
    // WebRTC API setup
    // -------------------------
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    let action_service = Arc::new(ActionService::new(cli.stty_path.as_path(), cli.baud_rate).await?);

    let state = Arc::new(AppState::new(api, action_service));

    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            state_clone.process_queue().await;
        }
    });

    rtp_thread(cli.rtp_addr, state.clone());

    // todo: add JWT protection

    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .route("/sdp", post(handle_sdp_offer))
        .nest_service("/static", ServeDir::new(web_dir))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cli.servet_addr.to_owned()).await?;

    info!("Server starting on http://{}", cli.servet_addr);

    #[cfg(feature = "gstream")]
    let video_handle = video_stream_start(cli.video_dev, cli.v8stream_url);

    tokio::select! {
        _ = axum::serve(listener, app) => {},
        _ = signal::ctrl_c() => {
            info!("Ctrl+C received, stopping streamer...");
            #[cfg(feature = "gstream")]
            {
                video_handle.thread().unpark();
            }
        }
    }
    Ok(())
}

async fn serve_index(jar: CookieJar) -> impl IntoResponse {
    let user_id = jar
        .get("user_id")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    // Create cookie: Set user_id to Cookie
    let cookie = Cookie::build(("user_id", user_id))
        .path("/")
        .secure(true)
        .http_only(true)
        .build();

    (jar.add(cookie), Html(include_str!("../web/index.html")))
}

