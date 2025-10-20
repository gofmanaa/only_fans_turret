use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    RequestAccess,
    Control { action: Action },
    ReleaseControl,
    GetUserId,
    UserDisconnected { user_id: String }, //todo: not implemented
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Right,
    Left,
    Up,
    Down,
    Fire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    AccessGranted { user_id: String },
    AccessDenied { user_id: String },
    QueuePosition { user_id: String, position: usize },
    ControlAction { user_id: String, action: Action },

    Error { user_id: String, message: String }, //todo: not implemented
    ResponseUserId { user_id: String },
}
