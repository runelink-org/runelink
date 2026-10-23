use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ids::ServerId;

pub use crate::ids::{ServerChannelId, TextChannelId};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "text", rename_all = "lowercase")
)]
pub enum ServerChannelKind {
    /// Owns exactly one underlying text channel.
    Text,
    /// Groups other server channels and owns no underlying channel.
    Category,
}

/// A channel in a server's category tree.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerChannel {
    pub id: ServerChannelId,
    pub server_id: ServerId,
    pub kind: ServerChannelKind,
    pub title: String,
    pub description: Option<String>,
    /// Present exactly when `kind` is `Text`.
    pub text_channel_id: Option<TextChannelId>,
    /// The parent category, or `None` at the server root.
    pub parent_channel_id: Option<ServerChannelId>,
    /// Ordering among siblings, by `position` then `id`.
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A new server channel, placed after its future siblings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewServerChannel {
    pub kind: ServerChannelKind,
    pub title: String,
    pub description: Option<String>,
    pub parent_channel_id: Option<ServerChannelId>,
}

/// Moves a server channel, with its subtree, to an index among the children
/// of `parent_channel_id`. Indices past the end place it last.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerChannelMove {
    pub parent_channel_id: Option<ServerChannelId>,
    pub index: u32,
}

impl ServerChannel {
    pub fn verbose(&self) -> String {
        format!("{} ({})", self.title, self.id)
    }

    /// The ID of the underlying channel this channel owns, if any.
    pub fn raw_id(&self) -> Option<Uuid> {
        match self.kind {
            ServerChannelKind::Text => {
                self.text_channel_id.map(|id| id.as_uuid())
            }
            ServerChannelKind::Category => None,
        }
    }
}

impl fmt::Display for ServerChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ServerChannelKind::Text => write!(f, "#{}", self.title),
            ServerChannelKind::Category => write!(f, "{}", self.title),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(kind: ServerChannelKind) -> ServerChannel {
        ServerChannel {
            id: ServerChannelId::new(),
            server_id: ServerId::new(),
            kind,
            title: "general".into(),
            description: None,
            text_channel_id: match kind {
                ServerChannelKind::Text => Some(TextChannelId::new()),
                ServerChannelKind::Category => None,
            },
            parent_channel_id: None,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn raw_id_is_the_underlying_channel_id() {
        let text = channel(ServerChannelKind::Text);
        assert_eq!(text.raw_id(), text.text_channel_id.map(|id| id.as_uuid()));
        assert_eq!(channel(ServerChannelKind::Category).raw_id(), None);
    }
}
