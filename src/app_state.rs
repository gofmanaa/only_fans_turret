use crate::devices::pb::device_client::DeviceClient;
use crate::message::ServerMessage;
use axum::extract::ws::Message;
use serde_json::to_string;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{info, warn};
use webrtc::rtp::packet::Packet;
use crate::config::WebConfig;

#[derive(Debug, Clone)]
pub struct UserSession {
    pub id: String,
    pub has_control: bool,
    pub control_granted_at: Option<Instant>,
    last_action_at: Option<Instant>,
    session_ttl: Duration,
}

const ACTION_COOLDOWN: Duration = Duration::from_millis(300);

impl UserSession {
    pub fn new(user_id: impl Into<String>, session_ttl_sec: u64) -> Self {
        Self {
            id: user_id.into(),
            has_control: false,
            control_granted_at: None,
            last_action_at: None,
            session_ttl: Duration::from_secs(session_ttl_sec),
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
        self.control_granted_at
            .map(|t| t.elapsed() > self.session_ttl)
            .unwrap_or(false)
    }

    pub fn can_do_action(&self) -> bool {
        match self.last_action_at {
            Some(last) => last.elapsed() >= ACTION_COOLDOWN,
            None => true, // no action yet -> allowed
        }
    }

    pub fn record_action(&mut self) {
        self.last_action_at = Some(Instant::now());
    }
}

// ===== Queue Management =====

#[derive(Debug, Default)]
pub struct AccessQueue {
    queue: VecDeque<String>,
    active_user: Option<String>,
}

impl AccessQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_user(&mut self, user_id: String) -> usize {
        if !self.queue.contains(&user_id) && self.active_user.as_deref() != Some(&user_id) {
            info!("User {} added to queue", user_id);
            self.queue.push_back(user_id);
        }
        self.queue.len()
    }

    pub fn remove_user(&mut self, user_id: &str) {
        self.queue.retain(|id| id != user_id);
        if self.active_user.as_deref() == Some(user_id) {
            info!("Active user {} removed", user_id);
            self.deactivate();
        }
    }

    pub fn next_user(&mut self) -> Option<String> {
        self.active_user = self.queue.pop_front();
        self.active_user.clone()
    }

    pub fn position(&self, user_id: &str) -> Option<usize> {
        self.queue
            .iter()
            .position(|id| id == user_id)
            .map(|p| p + 1)
    }

    pub fn active(&self) -> Option<&String> {
        self.active_user.as_ref()
    }

    pub fn deactivate(&mut self) {
        self.active_user = None
    }

    pub fn waiting_list(&self) -> Vec<String> {
        self.queue.iter().cloned().collect()
    }
}

// Application State
type UserWSSender = Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>;

pub struct AppState {
    pub users: Arc<RwLock<HashMap<String, UserSession>>>,
    pub queue: Arc<Mutex<AccessQueue>>,
    pub user_ws_senders: UserWSSender,
    pub rtp_broadcast: broadcast::Sender<Packet>,
    pub(crate) api: Arc<webrtc::api::API>,
    pub device_client: Arc<Mutex<DeviceClient<tonic::transport::Channel>>>,
    
    pub web_config: WebConfig,
}

impl AppState {
    pub fn new(
        api: Arc<webrtc::api::API>,
        device_client: Arc<Mutex<DeviceClient<tonic::transport::Channel>>>,
        web_config: WebConfig,
    ) -> Self {
        let (rtp_broadcast, _) = broadcast::channel(1000);
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(AccessQueue::new())),
            user_ws_senders: Arc::new(RwLock::new(HashMap::new())),
            rtp_broadcast,
            api,
            device_client,
            web_config,
        }
    }

    pub async fn add_user(&self, user: UserSession) {
        let user_id = user.id.clone();
        info!("User {} session added", &user_id);
        self.users.write().await.insert(user_id, user);
    }

    pub async fn get_user(&self, user_id: &str) -> Option<UserSession> {
        self.users.read().await.get(user_id).cloned()
    }

    pub async fn remove_user(&self, user_id: &str) {
        info!("User {} session removed", user_id);
        self.users.write().await.remove(user_id);
        self.queue.lock().await.remove_user(user_id);
        self.user_ws_senders.write().await.remove(user_id);
    }

    pub async fn send_message_to_user(&self, user_id: &str, message: ServerMessage) {
        if let Ok(json_message) = to_string(&message) {
            let mut senders = self.user_ws_senders.write().await;
            if let Some(sender) = senders.get_mut(user_id) {
                if sender
                    .send(Message::Text(json_message.into()))
                    .await
                    .is_err()
                {
                    warn!("Channel closed for user {}, removing sender", user_id);
                    senders.remove(user_id);
                }
            } else {
                warn!("No WebSocket sender found for {}", user_id);
            }
        }
    }

    pub async fn process_queue(&self) {
        let (active_id, next_user, waiting_users) = {
            let mut queue = self.queue.lock().await;
            let active_id = queue.active().cloned();
            let next_user = if active_id.is_none() {
                queue.next_user()
            } else {
                None
            };
            let waiting_users = queue.waiting_list();
            (active_id, next_user, waiting_users)
        };

        // Handle expired active user
        if let Some(ref active) = active_id {
            let expired = {
                let users = self.users.read().await;
                users.get(active).is_none_or(|u| u.is_control_expired())
            };
            if expired {
                info!("User {} expired, control revoked", active);
                self.queue.lock().await.deactivate();
                self.send_message_to_user(
                    active,
                    ServerMessage::AccessDenied {
                        user_id: active.clone(),
                    },
                )
                .await;
            }
        }

        // Grant next user control
        if let Some(next) = next_user
            && let Some(user) = self.users.write().await.get_mut(&next)
        {
            user.grant_control();
            info!("User {} granted control", next);
            self.send_message_to_user(
                &next,
                ServerMessage::AccessGranted {
                    user_id: next.clone(),
                },
            )
            .await;
        }

        // Notify waiting users about their queue position
        let positions: Vec<(String, usize)> = {
            let queue = self.queue.lock().await;
            waiting_users
                .into_iter()
                .filter_map(|uid| queue.position(&uid).map(|pos| (uid, pos)))
                .collect()
        };

        for (uid, pos) in positions {
            self.send_message_to_user(
                &uid,
                ServerMessage::QueuePosition {
                    user_id: uid.clone(),
                    position: pos,
                },
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app_state::AccessQueue;

    #[test]
    fn queue_add_user() {
        let mut queue = AccessQueue::new();
        let len = queue.add_user("sasha".to_string());
        assert_eq!(len, 1);
        assert_eq!(queue.active_user, None);
        let len = queue.add_user("alise".to_string());
        assert_eq!(len, 2);

        let position = queue.position("sasha").unwrap();
        assert_eq!(position, 1);

        let position = queue.position("alise").unwrap();
        assert_eq!(position, 2);

        let next = queue.next_user().unwrap();
        assert_eq!(queue.active(), Some(&next));
        assert_eq!(queue.active(), Some(&"sasha".to_string()));

        let next = queue.next_user().unwrap();
        assert_eq!(queue.active(), Some(&next));
        assert_eq!(queue.active(), Some(&"alise".to_string()));

        let _ = queue.next_user();
        assert_eq!(queue.active(), None);
    }

    #[test]
    fn queue_remove_user() {
        let mut queue = AccessQueue::new();
        queue.add_user("sasha".to_string());
        queue.add_user("bob".to_string());
        let len = queue.add_user("alise".to_string());
        assert_eq!(len, 3);

        assert_eq!(queue.position("sasha").unwrap(), 1);
        assert_eq!(queue.position("bob").unwrap(), 2);
        assert_eq!(queue.position("alise").unwrap(), 3);

        queue.remove_user("bob");
        assert_eq!(queue.position("sasha").unwrap(), 1);
        assert_eq!(queue.position("alise").unwrap(), 2);

        queue.remove_user("sasha");
        assert_eq!(queue.position("sasha"), None);
        assert_eq!(queue.position("alise").unwrap(), 1);

        assert_eq!(queue.active(), None);
    }
}
