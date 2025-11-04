use device::actions::Action;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    RequestAccess,
    Control { action: Action },
    ReleaseControl,
    GetUserId,
    UserDisconnected { user_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    AccessGranted { user_id: Uuid },
    AccessDenied { user_id: Uuid },
    QueuePosition { user_id: Uuid, position: usize },
    ControlAction { user_id: Uuid, action: Action },
    ResponseUserId { user_id: Uuid },
}
