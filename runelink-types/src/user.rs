use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

use crate::host::{Host, InvalidHost};

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
    pub host: Host,
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
    pub host: Host,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewUser {
    pub name: String,
    pub host: Host,
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
    pub fn new(
        name: String,
        host: impl AsRef<str>,
    ) -> Result<Self, InvalidHost> {
        Ok(Self {
            name,
            host: Host::parse(host.as_ref())?,
        })
    }

    /// Format as "name@host" for use in JWT subject claims.
    pub fn as_subject(&self) -> String {
        format!("{}@{}", self.name, self.host)
    }

    /// Parse "name@host" string into UserRef. Returns None if format is invalid.
    pub fn parse_subject(s: &str) -> Option<Self> {
        let (name, host) = s.split_once('@')?;
        let user_ref = Self::new(name.to_string(), host).ok()?;
        (!user_ref.name.is_empty()).then_some(user_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::{Host, User, UserRef};

    #[test]
    fn user_ref_hosts_are_canonical() {
        let expected = UserRef {
            name: "alice".into(),
            host: Host::parse("example.com").unwrap(),
        };
        assert_eq!(
            UserRef::new("alice".into(), "Example.COM:7000"),
            Ok(expected.clone())
        );
        assert!(UserRef::new("alice".into(), "x:7000:7000").is_err());
        let parsed: UserRef = serde_json::from_str(
            r#"{"name":"alice","host":"EXAMPLE.com:7000"}"#,
        )
        .unwrap();
        assert_eq!(parsed, expected);
        assert!(
            serde_json::from_str::<UserRef>(
                r#"{"name":"alice","host":"example.com/x"}"#
            )
            .is_err()
        );
        let user: User = serde_json::from_str(
            r#"{"name":"alice","host":"Example.com:7000","role":"user",
            "created_at":"2026-01-01T00:00:00Z",
            "updated_at":"2026-01-01T00:00:00Z","synced_at":null}"#,
        )
        .unwrap();
        assert_eq!(user.as_ref(), expected);
        assert_eq!(
            UserRef::parse_subject("alice@Example.com:7000"),
            Some(expected)
        );
        assert_eq!(UserRef::parse_subject("alice@:7000"), None);
        assert_eq!(UserRef::parse_subject("@example.com"), None);
    }
}
