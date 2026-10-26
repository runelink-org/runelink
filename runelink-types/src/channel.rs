use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

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
    #[serde(flatten)]
    pub content: ServerChannelContent,
    pub title: String,
    pub description: Option<String>,
    /// The parent category, or `None` at the server root.
    pub parent_channel_id: Option<ServerChannelId>,
    /// Ordering among siblings, by `position` then `id`.
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ServerChannelContent {
    Text { text_channel_id: TextChannelId },
    Category,
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
    pub fn kind(&self) -> ServerChannelKind {
        match self.content {
            ServerChannelContent::Text { .. } => ServerChannelKind::Text,
            ServerChannelContent::Category => ServerChannelKind::Category,
        }
    }

    pub fn verbose(&self) -> String {
        format!("{} ({})", self.title, self.id)
    }

    /// The ID of the underlying channel this channel owns, if any.
    pub fn raw_id(&self) -> Option<String> {
        match self.content {
            ServerChannelContent::Text { text_channel_id } => {
                Some(format!("text: {text_channel_id}"))
            }
            ServerChannelContent::Category => None,
        }
    }
}

impl fmt::Display for ServerChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.content {
            ServerChannelContent::Text { .. } => write!(f, "#{}", self.title),
            ServerChannelContent::Category => write!(f, "{}", self.title),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(content: ServerChannelContent) -> ServerChannel {
        ServerChannel {
            id: ServerChannelId::new(),
            server_id: ServerId::new(),
            content,
            title: "general".into(),
            description: None,
            parent_channel_id: None,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn content_is_flattened_and_round_trips() {
        use ServerChannelContent::{Category, Text};
        use serde_json::json;
        let text_channel_id = TextChannelId::new();
        for (content, expected) in [
            (
                Text { text_channel_id },
                json!({"kind": "text", "text_channel_id": text_channel_id}),
            ),
            (Category, json!({"kind": "category"})),
        ] {
            assert_eq!(serde_json::to_value(content).unwrap(), expected);
            let channel = channel(content);
            let mut wire = serde_json::to_value(&channel).unwrap();
            assert_eq!(wire["kind"], expected["kind"]);
            assert_eq!(
                wire.get("text_channel_id"),
                expected.get("text_channel_id")
            );
            assert!(wire.get("content").is_none());
            assert_eq!(
                serde_json::from_value::<ServerChannel>(wire.clone()).unwrap(),
                channel
            );
            if channel.kind() == ServerChannelKind::Text {
                wire.as_object_mut().unwrap().remove("text_channel_id");
                assert!(
                    serde_json::from_value::<ServerChannel>(wire.clone())
                        .is_err()
                );
                for id in [json!(null), json!("invalid")] {
                    wire["text_channel_id"] = id;
                    assert!(
                        serde_json::from_value::<ServerChannel>(wire.clone())
                            .is_err()
                    );
                }
            }
        }
        assert_eq!(
            channel(Text { text_channel_id }).raw_id(),
            Some(format!("text: {text_channel_id}"))
        );
        assert_eq!(channel(Category).raw_id(), None);
    }
}
