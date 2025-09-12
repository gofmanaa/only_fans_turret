mod app_state;
mod gst_v8_stream;
mod handler;
mod message;
mod rtp;
mod sdp_handler;

use std::path::PathBuf;
use crate::app_state::AppState;
use crate::gst_v8_stream::Vp8Streamer;
use crate::handler::websocket_handler;
use crate::rtp::rtp_thread;
use crate::sdp_handler::handle_sdp_offer;
use axum::Router;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use std::process;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_loki::url::Url;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use uuid::Uuid;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::interceptor::registry::Registry;

async fn serve_index(jar: CookieJar) -> impl IntoResponse {
    // let mut user_id = Uuid::new_v4().to_string();
    let user_id = jar.get("user_id").map(|c| c.value().to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    // Create cookie: Set user_id to Cookie
    let cookie = Cookie::build(("user_id", user_id))
        .path("/")
        .secure(true)
        .http_only(true)
        .build();

    (jar.add(cookie), Html(include_str!("../web/index.html")))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (layer, task) = tracing_loki::builder()
        .label("host", "mine")?
        .extra_field("pid", format!("{}", process::id()))?
        .http_header("X-Scope-OrgID", "tenant1")?
        .build_url(Url::parse("http://127.0.0.1:3100").unwrap())?;

    tracing_subscriber::registry()
        .with(layer)
        .with(tracing_subscriber::fmt::Layer::new())
        .with(EnvFilter::from_default_env())
        .init();

    // tracing_subscriber::fmt::init();
    tokio::spawn(task);

    let streamer = Arc::new(Mutex::new(Vp8Streamer::new(
        "/dev/video0",
        "127.0.0.1",
        5004,
    )?));

    let streamer_thread = Arc::clone(&streamer);

    let streamer_handle = tokio::spawn(async move {
        {
            let s = streamer_thread.lock().await;
            s.start().unwrap();
            info!("Streaming started...");
        }

        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

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

    let state = Arc::new(AppState::new(api));

    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            state_clone.process_queue().await;
        }
    });

    rtp_thread("127.0.0.1:5004".to_string(), state.clone());

    // todo: add JWT protection

    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .route("/sdp", post(handle_sdp_offer))
        .nest_service("/static", ServeDir::new(web_dir))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;

    info!("Server starting on http://127.0.0.1:8080");

    // axum::serve(listener, app).await?;

    tokio::select! {
        _ = axum::serve(listener, app) => {},
        _ = signal::ctrl_c() => {
            info!("Ctrl+C received, stopping streamer...");
            let s = streamer.lock().await;
            s.stop().unwrap();
            streamer_handle.abort();
        }
    }
    Ok(())
}
