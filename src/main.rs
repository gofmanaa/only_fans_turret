mod handler;
mod message;
mod app_state;
mod gst_v8_stream;
mod rtp;
mod sdp_handler;

use std::sync::Arc;
use std::time::Duration;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse};
use axum::Router;
use axum::routing::{get, post};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use tokio::signal;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;
use uuid::Uuid;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::interceptor::registry::Registry;
use crate::app_state::{AppState, UserSession};
use crate::gst_v8_stream::Vp8Streamer;
use crate::handler::websocket_handler;
use crate::rtp::rtp_thread;
use crate::sdp_handler::handle_sdp_offer;

async fn serve_index(jar: CookieJar) -> impl IntoResponse {
    let user_id = Uuid::new_v4().to_string();

    // Create cookie: Set user_id to Cookie
    let cookie = Cookie::build(("user_id", user_id.clone()))
        .path("/")
        .secure(true)
        .http_only(true)
        .build();

    (jar.add(cookie), Html(include_str!("../web/index.html")))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let streamer = Arc::new(Mutex::new(
        Vp8Streamer::new("/dev/video0", "127.0.0.1", 5004)?
    ));

    let streamer_thread = Arc::clone(&streamer);

    let streamer_handle = tokio::spawn(async move {
        {
            let s = streamer_thread.lock().await;
            s.start().unwrap();
            println!("Streaming started...");
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

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .route("/sdp", post(handle_sdp_offer))
        .nest_service("/static", ServeDir::new("web"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;

    info!("Server starting on http://127.0.0.1:3000");

   // axum::serve(listener, app).await?;

    tokio::select! {
        _ = axum::serve(listener, app) => {},
        _ = signal::ctrl_c() => {
            println!("Ctrl+C received, stopping streamer...");
            let s = streamer.lock().await;
            s.stop().unwrap();
            streamer_handle.abort();
        }
    }
    Ok(())
}