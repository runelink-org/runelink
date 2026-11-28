use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

use crate::{
    channel::ServerChannel,
    host::Host,
    user::{User, UserRef},
};

pub use crate::ids::{ServerId, ServerRef};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Server {
    pub id: ServerId,
    pub host: Host,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewServer {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Also create the default channels: a "general" text channel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default_channels: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerWithChannels {
    pub server: Server,
    pub channels: Vec<ServerChannel>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "server_role", rename_all = "lowercase")
)]
pub enum ServerRole {
    Member,
    Admin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerMembership {
    pub server: Server,
    pub user_ref: UserRef,
    pub role: ServerRole,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub synced_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FullServerMembership {
    pub server: Server,
    pub user: User,
    pub role: ServerRole,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub synced_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerMember {
    pub user: User,
    pub role: ServerRole,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewServerMembership {
    pub user_ref: UserRef,
    pub server_ref: ServerRef,
    pub role: ServerRole,
}

/// A new membership sent to the server's own host, with the user's profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewServerMembershipFull {
    pub user: User,
    pub server_id: ServerId,
    pub role: ServerRole,
}

impl Server {
    pub fn to_ref(&self) -> ServerRef {
        ServerRef::new(self.host.clone(), self.id)
    }

    pub fn verbose(&self) -> String {
        format!("{} ({})", self.title, self.id)
    }
}

impl fmt::Display for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.title)
    }
}

impl From<FullServerMembership> for ServerMembership {
    fn from(full_membership: FullServerMembership) -> Self {
        ServerMembership {
            server: full_membership.server,
            user_ref: full_membership.user.as_ref(),
            role: full_membership.role,
            joined_at: full_membership.joined_at,
            updated_at: full_membership.updated_at,
            synced_at: full_membership.synced_at,
        }
    }
}

impl From<FullServerMembership> for ServerMember {
    fn from(full_membership: FullServerMembership) -> Self {
        ServerMember {
            user: full_membership.user,
            role: full_membership.role,
            joined_at: full_membership.joined_at,
            updated_at: full_membership.updated_at,
        }
    }
}

impl ServerMembership {
    pub fn as_full(self, user: User) -> FullServerMembership {
        FullServerMembership {
            server: self.server,
            user,
            role: self.role,
            joined_at: self.joined_at,
            updated_at: self.updated_at,
            synced_at: self.synced_at,
        }
    }
}

impl NewServerMembership {
    pub fn as_full(self, user: User) -> NewServerMembershipFull {
        NewServerMembershipFull {
            user,
            server_id: self.server_ref.id,
            role: self.role,
        }
    }
}

impl NewServerMembershipFull {
    /// The membership on `host`, the host that received it, which is the
    /// server's host.
    pub fn with_host(self, host: Host) -> NewServerMembership {
        NewServerMembership {
            user_ref: self.user.as_ref(),
            server_ref: ServerRef::new(host, self.server_id),
            role: self.role,
        }
    }
}

/// Editable presentation fields with optimistic concurrency.
pub type ServerUpdate = crate::edit::DetailsUpdate;
