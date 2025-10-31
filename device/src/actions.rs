use crate::pb::Action as ProtoAction;
use serde::{Deserialize, Serialize};
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Right,
    Left,
    Up,
    Down,
    Fire,
}

impl From<ProtoAction> for Action {
    fn from(a: ProtoAction) -> Self {
        match a {
            ProtoAction::Right => Action::Right,
            ProtoAction::Left => Action::Left,
            ProtoAction::Up => Action::Up,
            ProtoAction::Down => Action::Down,
            ProtoAction::Fire => Action::Fire,
        }
    }
}

impl From<Action> for ProtoAction {
    fn from(a: Action) -> Self {
        match a {
            Action::Right => ProtoAction::Right,
            Action::Left => ProtoAction::Left,
            Action::Up => ProtoAction::Up,
            Action::Down => ProtoAction::Down,
            Action::Fire => ProtoAction::Fire,
        }
    }
}
