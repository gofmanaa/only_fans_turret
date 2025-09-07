mod handler;
mod message;
mod app_state;

use std::sync::Arc;
use std::time::Duration;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse};
use axum::Router;
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;
use uuid::Uuid;
use crate::app_state::{AppState, UserSession};
use crate::handler::websocket_handler;

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

    let state = Arc::new(AppState::new());

    // Spawn background task to process queue periodically
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            state_clone.process_queue().await;
        }
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(websocket_handler))
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;

    info!("Server starting on http://127.0.0.1:3000");

    axum::serve(listener, app).await?;

    Ok(())
}