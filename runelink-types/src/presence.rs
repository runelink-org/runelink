use serde::{Deserialize, Serialize};

use crate::user::UserRef;

/// A user's presence status, as other users see it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Online,
    DoNotDisturb,
    Offline,
}

/// The presence status a user chose for themselves while they are online.
/// Users are online again whenever they come back after being offline.
#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum ChosenPresenceStatus {
    #[default]
    Online,
    DoNotDisturb,
    /// Shown to other users as offline.
    Invisible,
}

impl ChosenPresenceStatus {
    /// The status other users see while the user is online.
    pub fn shown(self) -> PresenceStatus {
        match self {
            Self::Online => PresenceStatus::Online,
            Self::DoNotDisturb => PresenceStatus::DoNotDisturb,
            Self::Invisible => PresenceStatus::Offline,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserPresenceStatus {
    pub user_ref: UserRef,
    pub status: PresenceStatus,
}

/// A user's presence status, sent by a server's host to the users on the
/// receiving host who share one of its servers with them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayedPresenceStatus {
    pub user_ref: UserRef,
    pub status: PresenceStatus,
    pub recipients: Vec<UserRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_have_stable_wire_names() {
        assert_eq!(
            serde_json::to_value([
                PresenceStatus::Online,
                PresenceStatus::DoNotDisturb,
                PresenceStatus::Offline,
            ])
            .unwrap(),
            serde_json::json!(["online", "do_not_disturb", "offline"])
        );
        assert_eq!(
            serde_json::to_value(ChosenPresenceStatus::Invisible).unwrap(),
            "invisible"
        );
    }

    #[test]
    fn invisible_users_are_shown_offline() {
        assert_eq!(
            ChosenPresenceStatus::Invisible.shown(),
            PresenceStatus::Offline
        );
    }
}
