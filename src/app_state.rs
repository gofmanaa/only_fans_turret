// ===== User Session Management =====

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::info;
use uuid::Uuid;
use crate::message::ServerMessage;

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
        self.queue.iter().position(|id| id == user_id).map(|p| p + 1)
    }
}

// ===== Application State =====

#[derive(Debug)]
pub struct AppState {
    pub users: RwLock<HashMap<String, UserSession>>,
    pub queue: Mutex<AccessQueue>,
    pub broadcast_tx: broadcast::Sender<ServerMessage>,
}

impl AppState {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);
        Self {
            users: RwLock::new(HashMap::new()),
            queue: Mutex::new(AccessQueue::new()),
            broadcast_tx,
        }
    }

    pub async fn add_user(&self, user: UserSession) {
        let user_id = user.id.clone();
        info!("Added user's session {} ", &user_id);
        self.users.write().await.insert(user_id, user);

    }

    pub async fn remove_user(&self, user_id: &str) {
        info!("Removed user's session  {} ", user_id);
        self.users.write().await.remove(user_id);
        self.queue.lock().await.remove_user(user_id);

    }

    // pub async fn process_queue(&self) {
    //     let mut queue = self.queue.lock().await;
    //
    //     // Check if current active user's control has expired
    //     if let Some(active_id) = &queue.active_user.clone() {
    //         let users = self.users.read().await;
    //         if let Some(user) = users.get(active_id) {
    //             if user.is_control_expired() {
    //                 info!("User {} control expired, moving to next in queue", active_id);
    //                 queue.active_user = None;
    //             }
    //         } else {
    //             // User disconnected
    //             queue.active_user = None;
    //         }
    //     }
    //
    //     // Grant control to next user if no one has control
    //     if queue.active_user.is_none() {
    //         if let Some(next_user_id) = queue.get_next_user() {
    //             drop(queue); // Release queue lock before acquiring users lock
    //             let mut users = self.users.write().await;
    //             if let Some(user) = users.get_mut(&next_user_id) {
    //                 user.grant_control();
    //                 info!("Granted control to user {}", next_user_id);
    //
    //                 // Send access granted message (ignore errors for now)
    //                 let _ = self.broadcast_tx.send(ServerMessage::AccessGranted);
    //             }
    //         }
    //     } else {
    //         drop(queue);
    //     }
    //
    //     // Send queue positions to all waiting users
    //     let queue = self.queue.lock().await;
    //     for (i, _user_id) in queue.queue.iter().enumerate() {
    //         let message = ServerMessage::QueuePosition { position: i + 1 };
    //         let _ = self.broadcast_tx.send(message);
    //     }
    // }
    //
    // pub async fn process_queue(&self) { //tod:  works but slow
    //     // Step 1: Acquire queue lock briefly to check active user
    //     let next_user_opt = {
    //         let mut queue = self.queue.lock().await;
    //
    //         // Check if current active user's control has expired
    //         if let Some(active_id) = &queue.active_user {
    //             let users = self.users.read().await;
    //             let expired = match users.get(active_id) {
    //                 Some(user) => user.is_control_expired(),
    //                 None => true, // user disconnected
    //             };
    //             if expired {
    //                 info!("User {} control expired, moving to next in queue", active_id);
    //                 queue.active_user = None;
    //             }
    //         }
    //
    //         // If no active user, pop next user from queue
    //         if queue.active_user.is_none() {
    //             queue.get_next_user()
    //         } else {
    //             None
    //         }
    //     }; // queue lock released here
    //
    //     // Step 2: Grant control to next user if any
    //     if let Some(next_user_id) = next_user_opt {
    //         let mut users = self.users.write().await;
    //         if let Some(user) = users.get_mut(&next_user_id) {
    //             user.grant_control();
    //             info!("Granted control to user {}", next_user_id);
    //
    //             let _ = self.broadcast_tx.send(ServerMessage::AccessGranted);
    //         }
    //
    //         // Update active_user in queue after granting control
    //         let mut queue = self.queue.lock().await;
    //         queue.active_user = Some(next_user_id);
    //     }
    //
    //     // Step 3: Broadcast queue positions to all waiting users
    //     let queue = self.queue.lock().await;
    //     for (i, _user_id) in queue.queue.iter().enumerate() {
    //         let message = ServerMessage::QueuePosition { position: i + 1 };
    //         let _ = self.broadcast_tx.send(message);
    //     }
    // }

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
                info!("User {} control expired, moving to next in queue", active_id);
                let mut queue = self.queue.lock().await;
                queue.active_user = None;


                let _ = self.broadcast_tx.send(ServerMessage::AccessDenied { user_id: active_id });
            }
        }

        // Step 3: Grant control if there is a next user
        if let Some(next_user_id) = next_user_opt {
            let mut users = self.users.write().await;
            if let Some(user) = users.get_mut(&next_user_id) {
                user.grant_control();
                info!("Granted control to user {}", next_user_id);

                let response = ServerMessage::AccessGranted { user_id: next_user_id.clone() } ;
                let _ = self.broadcast_tx.send(response);
            }

            let mut queue = self.queue.lock().await;
            queue.active_user = Some(next_user_id);
        }

        // Step 4: Broadcast queue positions (waiting list)
        for (i, user_id) in waiting_users.iter().enumerate() {
            let message = ServerMessage::QueuePosition { user_id: user_id.clone(), position: i + 1 };
            let _ = self.broadcast_tx.send(message);
        }
    }

}

