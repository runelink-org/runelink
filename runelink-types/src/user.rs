use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "user_role", rename_all = "lowercase")
)]
pub enum UserRole {
    User,
    Admin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct User {
    pub name: String,
    pub host: String,
    pub role: UserRole,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub synced_at: Option<OffsetDateTime>,
}

/// User identity: (name, host) pair used for identification and authorization.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserRef {
    pub name: String,
    pub host: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewUser {
    pub name: String,
    pub host: String,
    pub role: UserRole,
}

impl User {
    pub fn as_ref(&self) -> UserRef {
        UserRef {
            name: self.name.clone(),
            host: self.host.clone(),
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.host)
    }
}

impl From<User> for UserRef {
    fn from(user: User) -> Self {
        UserRef {
            name: user.name,
            host: user.host,
        }
    }
}

impl From<&User> for UserRef {
    fn from(user: &User) -> Self {
        UserRef {
            name: user.name.clone(),
            host: user.host.clone(),
        }
    }
}

impl fmt::Display for UserRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.host)
    }
}

impl UserRef {
    pub fn new(name: String, host: String) -> Self {
        Self { name, host }
    }

    /// Format as "name@host" for use in JWT subject claims.
    pub fn as_subject(&self) -> String {
        format!("{}@{}", self.name, self.host)
    }

    /// Parse "name@host" string into UserRef. Returns None if format is invalid.
    pub fn parse_subject(s: &str) -> Option<Self> {
        let (name, host) = s.split_once('@')?;
        if name.is_empty() || host.is_empty() {
            return None;
        }
        Some(Self {
            name: name.to_string(),
            host: host.to_string(),
        })
    }
}
