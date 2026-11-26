use serde::{Deserialize, Deserializer, Serialize};
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
    #[serde(deserialize_with = "deserialize_canonical_host")]
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
///
/// The host is canonicalized when a ref is created with [`UserRef::new`] or
/// deserialized, as is a deserialized [`User`]'s, so refs can be compared
/// exactly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserRef {
    pub name: String,
    #[serde(deserialize_with = "deserialize_canonical_host")]
    pub host: String,
}

/// The canonical spelling of a host: lowercase, without the default port.
pub fn canonical_host(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    match host.strip_suffix(":7000") {
        Some(stripped) => stripped.to_string(),
        None => host,
    }
}

fn deserialize_canonical_host<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|host| canonical_host(&host))
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
        Self {
            name,
            host: canonical_host(&host),
        }
    }

    /// Format as "name@host" for use in JWT subject claims.
    pub fn as_subject(&self) -> String {
        format!("{}@{}", self.name, self.host)
    }

    /// Parse "name@host" string into UserRef. Returns None if format is invalid.
    pub fn parse_subject(s: &str) -> Option<Self> {
        let (name, host) = s.split_once('@')?;
        let user_ref = Self::new(name.to_string(), host.to_string());
        (!user_ref.name.is_empty() && !user_ref.host.is_empty())
            .then_some(user_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::{User, UserRef};

    #[test]
    fn user_ref_hosts_are_canonical() {
        let expected = UserRef {
            name: "alice".into(),
            host: "example.com".into(),
        };
        assert_eq!(
            UserRef::new("alice".into(), "Example.COM:7000".into()),
            expected
        );
        let parsed: UserRef = serde_json::from_str(
            r#"{"name":"alice","host":"EXAMPLE.com:7000"}"#,
        )
        .unwrap();
        assert_eq!(parsed, expected);
        let user: User = serde_json::from_str(
            r#"{"name":"alice","host":"Example.com:7000","role":"user",
            "created_at":"2026-01-01T00:00:00Z",
            "updated_at":"2026-01-01T00:00:00Z","synced_at":null}"#,
        )
        .unwrap();
        assert_eq!(user.as_ref(), expected);
        assert_eq!(
            UserRef::new("alice".into(), "Example.com:7001".into()).host,
            "example.com:7001"
        );
        assert_eq!(UserRef::parse_subject("alice@:7000"), None);
    }
}
