use crate::message::ServerMessage;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{info, warn};
use webrtc::rtp::packet::Packet;
use axum::extract::ws::Message;
use serde_json::to_string;

#[derive(Debug, Clone)]
pub struct UserSession {
    pub id: String,
    pub joined_at: Instant,
    pub has_control: bool,
    pub control_granted_at: Option<Instant>,
}

const USER_SESSION_TIMEOUT: Duration = Duration::from_secs((0.5 * 60.0) as u64);

impl UserSession {
    pub fn new(user_id: String) -> Self {
        Self {
            id: user_id, //Uuid::new_v4().to_string(),
            joined_at: Instant::now(),
            has_control: false,
            control_granted_at: None,
        }
    }

    pub fn grant_control(&mut self) {
        self.has_control = true;
        self.control_granted_at = Some(Instant::now());
    }

    pub fn revoke_control(&mut self) {
        self.has_control = false;
        self.control_granted_at = None;
    }

    pub fn is_control_expired(&self) -> bool {
        if let Some(granted_at) = self.control_granted_at {
            granted_at.elapsed() > USER_SESSION_TIMEOUT // 5 minutes
        } else {
            false
        }
    }
}

// ===== Queue Management =====

#[derive(Debug)]
pub struct AccessQueue {
    pub queue: VecDeque<String>,
    pub active_user: Option<String>,
}

impl AccessQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            active_user: None,
        }
    }

    pub fn add_user(&mut self, user_id: String) -> usize {
        if !self.queue.contains(&user_id) && self.active_user.as_ref() != Some(&user_id) {
            info!("Adding user {} to queue", user_id);
            self.queue.push_back(user_id);
        }
        self.queue.len()
    }

    pub fn remove_user(&mut self, user_id: &str) {
        let user_id = user_id.to_string();
        self.queue.retain(|id| id.ne(&user_id));
        if self.active_user.as_ref() == Some(&user_id) {
            info!("Removing user {} from queue", user_id);
            self.active_user = None;
        }
    }

    pub fn get_next_user(&mut self) -> Option<String> {
        self.active_user = self.queue.pop_front();
        self.active_user.clone()
    }

    pub fn get_position(&self, user_id: &str) -> Option<usize> {
        self.queue
            .iter()
            .position(|id| id == user_id)
            .map(|p| p + 1)
    }
}

// ===== Application State =====
type UserWSSender = Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>;
pub struct AppState {
    pub users: Arc<RwLock<HashMap<String, UserSession>>>,
    pub queue: Arc<Mutex<AccessQueue>>,
    pub user_ws_senders: UserWSSender,

    // video stream
    pub rtp_broadcast: broadcast::Sender<Packet>,

    pub(crate) api: Arc<webrtc::api::API>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            users: Arc::clone(&self.users),
            queue: Arc::clone(&self.queue),
            user_ws_senders: Arc::clone(&self.user_ws_senders),
            rtp_broadcast: self.rtp_broadcast.clone(),
            api: Arc::clone(&self.api),
        }
    }
}

impl AppState {
    pub fn new(api: webrtc::api::API) -> Self {
        let (rtp_broadcast, _) = broadcast::channel(1000); // Buffer for 1000 RTP packets
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(AccessQueue::new())),
            user_ws_senders: Arc::new(RwLock::new(HashMap::new())),
            rtp_broadcast,
            api: Arc::new(api),
        }
    }

    pub async fn add_user(&self, user: UserSession) {
        let user_id = user.id.clone();
        info!("Added user's session {} ", &user_id);
        self.users.write().await.insert(user_id, user);
    }

    pub async fn get_user<U: Into<String>>(&self, user_id: U) -> Option<UserSession> {
        let user_id: String = user_id.into();
        let users = self.users.read().await;
        users.get(&user_id).cloned()
    }

    pub async fn remove_user(&self, user_id: &str) {
        info!("Removed user's session  {} ", user_id);
        self.users.write().await.remove(user_id);
        self.queue.lock().await.remove_user(user_id);
        self.user_ws_senders.write().await.remove(user_id);
    }

    pub async fn send_message_to_user(&self, user_id: &str, message: ServerMessage) {
        let json_message = match to_string(&message) {
            Ok(json) => json,
            Err(e) => {
                warn!("Failed to serialize ServerMessage for user {}: {}", user_id, e);
                return;
            }
        };

        let mut senders = self.user_ws_senders.write().await;
        if let Some(sender) = senders.get_mut(user_id) {
            if let Err(e) = sender.send(Message::Text(json_message.into())).await {
                warn!("Failed to send message to user {}: {}", user_id, e);
                // Consider removing the sender if the channel is closed
                senders.remove(user_id);
            }
        } else {
            warn!("No WebSocket sender found for user {}", user_id);
        }
    }

    pub async fn process_queue(&self) {
        // Step 1: Determine what to do while holding queue lock
        let (expired_user_opt, next_user_opt, waiting_users) = {
            let mut queue = self.queue.lock().await;

            let mut expired_user_opt = None;
            if let Some(active_id) = &queue.active_user {
                // Just capture the active user id, check later outside the lock
                expired_user_opt = Some(active_id.clone());
            }

            // If no active user right now, check queue head
            let next_user_opt = if queue.active_user.is_none() {
                queue.get_next_user()
            } else {
                None
            };

            let waiting_users: Vec<String> = queue.queue.iter().cloned().collect();

            (expired_user_opt, next_user_opt, waiting_users)
        }; // queue lock released here

        // Step 2: Check if expired user really lost control
        if let Some(active_id) = expired_user_opt {
            let expired = {
                let users = self.users.read().await;
                match users.get(&active_id) {
                    Some(user) => user.is_control_expired(),
                    None => true, // disconnected
                }
            };

            if expired {
                info!(
                    "User {} control expired, moving to next in queue",
                    active_id
                );
                let mut queue = self.queue.lock().await;
                queue.active_user = None;

                self.send_message_to_user(&active_id, ServerMessage::AccessDenied { user_id: active_id.clone() }).await;
            }
        }

        // Step 3: Grant control if there is a next user
        if let Some(next_user_id) = next_user_opt {
            let mut users = self.users.write().await;
            if let Some(user) = users.get_mut(&next_user_id) {
                user.grant_control();
                info!("Granted control to user {}", next_user_id);

                self.send_message_to_user(&next_user_id, ServerMessage::AccessGranted { user_id: next_user_id.clone() }).await;
            }

            let mut queue = self.queue.lock().await;
            queue.active_user = Some(next_user_id);
        }

        // Step 4: Broadcast queue positions (waiting list)
        for (i, user_id) in waiting_users.iter().enumerate() {
            let message = ServerMessage::QueuePosition {
                user_id: user_id.clone(),
                position: i + 1,
            };
            self.send_message_to_user(user_id, message).await;
        }
    }
}
