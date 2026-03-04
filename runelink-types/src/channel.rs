use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

use crate::ids::ServerId;

pub use crate::ids::ChannelId;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Channel {
    pub id: ChannelId,
    pub server_id: ServerId,
    pub title: String,
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewChannel {
    pub title: String,
    pub description: Option<String>,
}

impl Channel {
    pub fn verbose(&self) -> String {
        format!("{} ({})", self.title, self.id)
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(desc) = &self.description {
            write!(f, "#{} - {}", self.title, desc)
        } else {
            write!(f, "#{}", self.title)
        }
    }
}
