use device::actions::Action;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    RequestAccess,
    Control { action: Action },
    ReleaseControl,
    GetUserId,
    UserDisconnected { user_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    AccessGranted { user_id: String },
    AccessDenied { user_id: String },
    QueuePosition { user_id: String, position: usize },
    ControlAction { user_id: String, action: Action },
    ResponseUserId { user_id: String },
}
