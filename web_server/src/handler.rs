use std::sync::Arc;

use crate::app_state::{AppState, UserSession};
use device::pb::{Action as PbAction, CommandRequest};
use crate::message::{ClientMessage, ServerMessage};
use axum::response::Html;
use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use axum::http::StatusCode;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub async fn serve_index(jar: CookieJar) -> impl IntoResponse {
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

// ===== WebSocket Handler =====

pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    // Get user_id from Cookie
    info!("Websocket handling connection");
    let Some(cookie) = jar.get("user_id") else {
        warn!("Missing user_id cookie in WebSocket request");
        return (
            StatusCode::UNAUTHORIZED,
            "Missing user_id cookie",
        )
            .into_response();
    };

    let Ok(user_uuid) = Uuid::parse_str(cookie.value()) else {
        warn!("Invalid UUID in user_id cookie: {}", cookie.value());
        return (
            StatusCode::BAD_REQUEST,
            "Invalid user_id cookie value",
        )
            .into_response();
    };
    ws.on_upgrade(move |socket| handle_websocket(socket, state, user_uuid))
}

async fn handle_websocket(socket: WebSocket, state: Arc<AppState>, user_id: Uuid) {
    let user_session = UserSession::new(user_id, state.web_config.controller_ttl);
    info!("New WebSocket connection: {}", user_id);

    state.add_user(user_session).await;

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(100); // Channel for sending messages to this specific user

    // Store the sender for this user
    state
        .user_ws_senders
        .write()
        .await
        .insert(user_id, tx);

    // Spawn task to handle outgoing messages to client (from the mpsc channel)
    let outgoing_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_sender.send(msg).await {
                warn!(
                    "Failed to send message to user {}: {}",
                    user_id, e
                );
                break;
            }
        }
    });

    // Spawn task to handle incoming messages from client
    let state_clone = state.clone();
    let incoming_task = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received text message: {}", text);
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        handle_client_message(
                            client_msg,
                            user_id,
                            &state_clone,
                        )
                        .await;
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by client: {}", user_id);
                    break;
                }
                Err(e) => {
                    warn!("WebSocket error for {}: {}", user_id, e);
                    break;
                }
                _ => {}
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

async fn handle_client_message(message: ClientMessage, user_id: Uuid, state: &Arc<AppState>) {
    match message {
        ClientMessage::RequestAccess => {
            let position = {
                let mut queue = state.queue.lock().await;
                queue.add_user(user_id)
            };
            info!(
                "User {} requested access, position in queue: {}",
                user_id, position
            );

            state
                .send_message_to_user(
                    &user_id,
                    ServerMessage::QueuePosition {
                        user_id,
                        position,
                    },
                )
                .await;

            state.process_queue().await;
        }

        ClientMessage::Control { action } => {
            let mut users = state.users.write().await; // write, not read
            if let Some(user) = users.get_mut(&user_id) {
                if user.has_control && !user.is_control_expired() {
                    info!(
                        "User has control: {}; can do action: {:?}",
                        user.has_control,
                        user.can_do_action()
                    );

                    if user.can_do_action() {
                        user.record_action();

                        let mut client = state.device_client.lock().await;
                        let pb_action = PbAction::from(action);
                        let request = tonic::Request::new(CommandRequest {
                            action: pb_action as i32,
                        });
                        match client.do_action(request).await {
                            Ok(r) => {
                                info!("GRPS response: {:?}", r.into_inner().action().as_str_name());
                            }
                            Err(e) => {
                                error!("Grpc error: {}", e);
                                return;
                            }
                        };
                    }

                    state
                        .send_message_to_user(
                            &user_id,
                            ServerMessage::ControlAction {
                                user_id,
                                action,
                            },
                        )
                        .await;
                } else {
                    warn!("User {} attempted control without permission", user_id);

                    state
                        .send_message_to_user(
                            &user_id,
                            ServerMessage::AccessDenied {
                                user_id,
                            },
                        )
                        .await;
                }
            }
        }

        ClientMessage::ReleaseControl => {
            let should_process = {
                let mut users = state.users.write().await;
                users
                    .get_mut(&user_id)
                    .filter(|u| u.has_control)
                    .map(|u| {
                        u.revoke_control();
                        info!("User {} released control", user_id);
                    })
                    .is_some()
            };

            if should_process {
                {
                    let mut queue = state.queue.lock().await;
                    queue.deactivate();
                }
                state.process_queue().await;
            }
        }

        ClientMessage::GetUserId => {
            let users = state.users.read().await;
            if let Some(user) = users.get(&user_id) {
                state
                    .send_message_to_user(
                        &user_id,
                        ServerMessage::ResponseUserId {
                            user_id: user.id,
                        },
                    )
                    .await;
                info!("ResponseUserId {}", user_id);
            }
        }

        ClientMessage::UserDisconnected { user_id } => {
            info!("User {} disconnected", user_id);
            state.remove_user(&user_id).await;
        }
    }
}
