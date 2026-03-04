use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

use crate::{
    ids::ChannelId,
    user::{User, UserRef},
};

pub use crate::ids::MessageId;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub author: Option<User>,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMessage {
    pub author: UserRef,
    pub body: String,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.author
                .as_ref()
                .map(|u| u.name.as_str())
                .unwrap_or("anon"),
            self.body
        )
    }
}
