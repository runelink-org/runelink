use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

use crate::{
    ids::TextChannelId,
    user::{User, UserRef},
};

pub use crate::ids::MessageId;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub text_channel_id: TextChannelId,
    pub author: Option<User>,
    pub reply_to: Option<MessageId>,
    #[serde(flatten)]
    pub content: MessageContent,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub edited_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMessage {
    pub author: UserRef,
    pub body: String,
    #[serde(default)]
    pub reply_to: Option<MessageId>,
}

/// The wire format has nullable `body` and `deleted_at` fields. Parsing validates
/// their relationship once, so consumers cannot observe an inconsistent state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageContent {
    Active {
        body: String,
        deleted_at: (),
    },
    Deleted {
        body: (),
        #[serde(with = "time::serde::rfc3339")]
        deleted_at: OffsetDateTime,
    },
}

impl MessageContent {
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Active { body, .. } => Some(body),
            Self::Deleted { .. } => None,
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(reply_to) = self.reply_to {
            write!(f, "[reply to {reply_to}] ")?;
        }
        write!(
            f,
            "{}: {}",
            self.author
                .as_ref()
                .map(|u| u.name.as_str())
                .unwrap_or("anon"),
            self.content.body().unwrap_or("[Message deleted]")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn wire_message(body: Value, deleted_at: Value) -> Value {
        json!({
            "id": MessageId::new(),
            "text_channel_id": TextChannelId::new(),
            "author": null,
            "reply_to": MessageId::new(),
            "body": body,
            "deleted_at": deleted_at,
            "created_at": "2026-10-19T12:00:00Z",
            "edited_at": null,
            "updated_at": "2026-10-19T12:00:00Z"
        })
    }

    #[test]
    fn message_content_states_round_trip() {
        for wire in [
            wire_message(json!("hello"), Value::Null),
            wire_message(Value::Null, json!("2026-10-19T12:00:01Z")),
        ] {
            let message: Message =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(message).unwrap(), wire);
        }
    }

    #[test]
    fn invalid_content_states_are_rejected() {
        for wire in [
            wire_message(Value::Null, Value::Null),
            wire_message(json!("deleted text"), json!("2026-10-19T12:00:01Z")),
        ] {
            assert!(serde_json::from_value::<Message>(wire).is_err());
        }
    }
}

/// An edit never changes identity, authorship, channel, or reply target.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageUpdate {
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expected_updated_at: OffsetDateTime,
}
