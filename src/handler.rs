use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app_state::{AppState, UserSession};
use crate::message::{ClientMessage, ServerMessage};
use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws::WebSocket},
    response::{Html, IntoResponse},
    routing::get,
};
use axum_extra::extract::CookieJar;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, broadcast};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// ===== WebSocket Handler =====

pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    // Get user_id from Cookie
    let user_id = jar.get("user_id").map(|c| c.value().to_string()).unwrap();
    ws.on_upgrade(|socket| handle_websocket(socket, state, user_id))
}

async fn handle_websocket(socket: WebSocket, state: Arc<AppState>, user_id: String) {
    let user_session = UserSession::new(user_id.clone(), None);
    // let user_id = user_session.id.clone();

    info!("New WebSocket connection: {}", user_id);

    state.add_user(user_session).await;

    let mut broadcast_rx = state.broadcast_tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Spawn task to handle incoming messages from client
    let state_clone = state.clone();
    let user_id_clone = user_id.clone();
    let incoming_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    debug!("Received text message: {}", text);
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        handle_client_message(client_msg, &user_id_clone, &state_clone).await;
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => {
                    info!("WebSocket closed by client: {}", user_id_clone);
                    break;
                }
                Err(e) => {
                    warn!("WebSocket error for {}: {}", user_id_clone, e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Spawn task to handle outgoing messages to client
    let outgoing_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if let Err(_) = sender
                    .send(axum::extract::ws::Message::Text(json.into()))
                    .await
                {
                    break;
                }
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = incoming_task => {},
        _ = outgoing_task => {},
    }

    // Cleanup
    state.remove_user(&user_id).await;
    state.process_queue().await;

    info!("WebSocket connection closed: {}", user_id);
}

async fn handle_client_message(message: ClientMessage, user_id: &str, state: &Arc<AppState>) {
    match message {
        ClientMessage::RequestAccess => {
            let position = {
                let mut queue = state.queue.lock().await;
                queue.add_user(user_id.to_string())
            };
            info!(
                "User {} requested access, position in queue: {}",
                user_id, position
            );

            let response = ServerMessage::QueuePosition {
                user_id: user_id.to_string(),
                position,
            };
            let _ = state.broadcast_tx.send(response);

            //drop(queue);
            state.process_queue().await;
        }

        ClientMessage::Control { action } => {
            let users = state.users.read().await;
            if let Some(user) = users.get(user_id) {
                if user.has_control && !user.is_control_expired() {
                    info!("User {} performed control action: {}", user_id, action);

                    let response = ServerMessage::ControlAction {
                        user_id: user_id.to_string(),
                        action,
                    };
                    let _ = state.broadcast_tx.send(response);
                } else {
                    warn!("User {} attempted control without permission", user_id);

                    let _ = state.broadcast_tx.send(ServerMessage::AccessDenied {
                        user_id: user_id.to_string(),
                    });
                }
            }
        }

        ClientMessage::ReleaseControl => {
            let mut users = state.users.write().await;
            if let Some(user) = users.get_mut(user_id) {
                if user.has_control {
                    user.revoke_control();
                    info!("User {} released control", user_id);

                    let mut queue = state.queue.lock().await;
                    queue.active_user = None;
                    drop(queue);
                    drop(users);

                    state.process_queue().await;
                }
            }
        }

        ClientMessage::GetUserId => {
            let users = state.users.read().await;
            if let Some(user) = users.get(user_id) {
                let response = ServerMessage::ResponseUserId {
                    user_id: user.id.clone(),
                };
                info!("ResponseUserId {}", user_id);
                let _ = state.broadcast_tx.send(response);
            }
        }
    }
}
