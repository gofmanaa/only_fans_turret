
// ===== Message Types =====

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    RequestAccess,
    Control { action: String }, // todo: change to enum type Action
    ReleaseControl,
    GetUserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    AccessGranted { user_id: String},
    AccessDenied { user_id: String},
    QueuePosition { user_id: String, position: usize },
    ControlAction { user_id: String, action: String },
    UserDisconnected { user_id: String }, //todo: not implemented
    Error {user_id: String, message: String }, //todo: not implemented
    ResponseUserId { user_id: String},
}
