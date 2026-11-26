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
/// The host is parsed and canonicalized when a ref is created with
/// [`UserRef::new`] or deserialized, so refs can be compared exactly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserRef {
    pub name: String,
    #[serde(deserialize_with = "deserialize_canonical_host")]
    pub host: String,
}

/// A host that is not a host name or IP address with an optional port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidHost(pub String);

impl fmt::Display for InvalidHost {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid host: {:?}", self.0)
    }
}

impl std::error::Error for InvalidHost {}

/// Parses a host name or IP address with an optional port into its canonical
/// spelling: lowercase, without the default port.
pub fn canonical_host(host: &str) -> Result<String, InvalidHost> {
    let invalid = || InvalidHost(host.to_string());
    let (name, port) = match host.rsplit_once(':') {
        // The last colon of an unported IPv6 address is inside its brackets.
        Some((name, port)) if !port.contains(']') => (name, Some(port)),
        _ => (host, None),
    };
    // URL hosts may also be percent-encoded, have empty labels, or contain
    // characters such as `!` and `_`.
    let parsed = url::Host::parse(name).map_err(|_| invalid())?;
    if name.contains('%')
        || matches!(&parsed, url::Host::Domain(domain)
            if !domain.split('.').all(is_host_label))
    {
        return Err(invalid());
    }
    let port = match port {
        Some(port) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            Some(port.parse::<u16>().map_err(|_| invalid())?)
        }
        Some(_) => return Err(invalid()),
        None => None,
    };
    Ok(match port {
        None | Some(7000) => parsed.to_string(),
        Some(port) => format!("{parsed}:{port}"),
    })
}

fn is_host_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn deserialize_canonical_host<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    canonical_host(&String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
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
    pub fn new(name: String, host: String) -> Result<Self, InvalidHost> {
        Ok(Self {
            name,
            host: canonical_host(&host)?,
        })
    }

    /// Format as "name@host" for use in JWT subject claims.
    pub fn as_subject(&self) -> String {
        format!("{}@{}", self.name, self.host)
    }

    /// Parse "name@host" string into UserRef. Returns None if format is invalid.
    pub fn parse_subject(s: &str) -> Option<Self> {
        let (name, host) = s.split_once('@')?;
        let user_ref = Self::new(name.to_string(), host.to_string()).ok()?;
        (!user_ref.name.is_empty()).then_some(user_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::{User, UserRef, canonical_host};

    #[test]
    fn canonical_host_parses_hosts() {
        for (host, expected) in [
            ("runelink.chat", Some("runelink.chat")),
            ("runelink.chat:7001", Some("runelink.chat:7001")),
            ("localhost", Some("localhost")),
            ("localhost:7001", Some("localhost:7001")),
            ("LocalHost:7000", Some("localhost")),
            ("localhost:07000", Some("localhost")),
            ("localhost:80", Some("localhost:80")),
            ("127.0.0.1:7001", Some("127.0.0.1:7001")),
            ("[::1]", Some("[::1]")),
            ("[0:0::1]:7000", Some("[::1]")),
            ("[::1]:7001", Some("[::1]:7001")),
            ("", None),
            (":7000", None),
            ("localhost:", None),
            ("localhost:+7001", None),
            ("localhost:70000", None),
            ("x:7000:7000", None),
            ("::1", None),
            ("local host", None),
            ("bad!name.example", None),
            ("bad_name.example", None),
            ("bad..example", None),
            ("-bad.example", None),
            ("example.com.", None),
            ("%65xample.com", None),
            ("localhost/path", None),
            ("alice@localhost", None),
            ("http://localhost", None),
        ] {
            assert_eq!(
                canonical_host(host).ok().as_deref(),
                expected,
                "{host}"
            );
        }
    }

    #[test]
    fn user_ref_hosts_are_canonical() {
        let expected = UserRef {
            name: "alice".into(),
            host: "example.com".into(),
        };
        assert_eq!(
            UserRef::new("alice".into(), "Example.COM:7000".into()),
            Ok(expected.clone())
        );
        assert!(UserRef::new("alice".into(), "x:7000:7000".into()).is_err());
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
