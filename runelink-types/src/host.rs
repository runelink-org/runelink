use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};

/// The port of a host that names no port.
pub const DEFAULT_PORT: u16 = 7000;

/// A host name or IP address with an optional port, in its canonical
/// spelling: lowercase, without the default port.
///
/// Hosts are only created by parsing, so two hosts name the same host exactly
/// when they are equal.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Host(String);

/// A host that is not a host name or IP address with an optional port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidHost(pub String);

impl fmt::Display for InvalidHost {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid host: {:?}", self.0)
    }
}

impl std::error::Error for InvalidHost {}

impl Host {
    /// Parses a host name or IP address with an optional port.
    pub fn parse(host: &str) -> Result<Self, InvalidHost> {
        canonical_host(host).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The host with its port, even the default one, for connecting to it.
    pub fn authority(&self) -> String {
        // An unported IPv6 address ends with its closing bracket.
        match self.0.rsplit_once(':') {
            Some((_, port)) if !port.contains(']') => self.0.clone(),
            _ => format!("{}:{DEFAULT_PORT}", self.0),
        }
    }

    /// The base URL of the host's HTTP API, which also names it in tokens.
    pub fn api_url(&self, secure: bool) -> String {
        let scheme = if secure { "https" } else { "http" };
        format!("{scheme}://{}", self.authority())
    }

    /// The host whose API URL is exactly `url`.
    pub fn from_api_url(url: &str, secure: bool) -> Option<Self> {
        let scheme = if secure { "https://" } else { "http://" };
        let host = Self::parse(url.strip_prefix(scheme)?).ok()?;
        (host.api_url(secure) == url).then_some(host)
    }

    pub fn client_ws_url(&self, secure: bool) -> String {
        self.ws_url(secure, "client")
    }

    pub fn federation_ws_url(&self, secure: bool) -> String {
        self.ws_url(secure, "federation")
    }

    fn ws_url(&self, secure: bool, kind: &str) -> String {
        let scheme = if secure { "wss" } else { "ws" };
        format!("{scheme}://{}/ws/{kind}", self.authority())
    }
}

impl From<Host> for String {
    fn from(host: Host) -> Self {
        host.0
    }
}

impl AsRef<str> for Host {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for Host {
    type Err = InvalidHost;

    fn from_str(host: &str) -> Result<Self, Self::Err> {
        Self::parse(host)
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

// Stored as text, and parsed again when read, like hosts from the wire.
#[cfg(feature = "sqlx")]
mod sqlx_impls {
    use super::Host;
    use sqlx::{
        Database, Decode, Encode, Type, encode::IsNull, error::BoxDynError,
    };

    impl<DB: Database> Type<DB> for Host
    where
        String: Type<DB>,
    {
        fn type_info() -> DB::TypeInfo {
            <String as Type<DB>>::type_info()
        }

        fn compatible(ty: &DB::TypeInfo) -> bool {
            <String as Type<DB>>::compatible(ty)
        }
    }

    impl<'r, DB: Database> Decode<'r, DB> for Host
    where
        String: Decode<'r, DB>,
    {
        fn decode(value: DB::ValueRef<'r>) -> Result<Self, BoxDynError> {
            Ok(Host::parse(&String::decode(value)?)?)
        }
    }

    impl<'q, DB: Database> Encode<'q, DB> for Host
    where
        String: Encode<'q, DB>,
    {
        fn encode_by_ref(
            &self,
            buf: &mut DB::ArgumentBuffer,
        ) -> Result<IsNull, BoxDynError> {
            self.0.encode_by_ref(buf)
        }
    }
}

/// Parses a host name or IP address with an optional port into its canonical
/// spelling: lowercase, without the default port.
fn canonical_host(host: &str) -> Result<String, InvalidHost> {
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
        None | Some(DEFAULT_PORT) => parsed.to_string(),
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

#[cfg(test)]
mod tests {
    use super::{Host, canonical_host};

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
    fn hosts_name_their_urls_with_ports() {
        for (host, authority) in [
            ("example.com", "example.com:7000"),
            ("example.com:8080", "example.com:8080"),
            ("[::1]", "[::1]:7000"),
            ("[::1]:4321", "[::1]:4321"),
        ] {
            let host = Host::parse(host).unwrap();
            assert_eq!(host.authority(), authority);
            assert_eq!(host.api_url(false), format!("http://{authority}"));
            assert_eq!(
                Host::from_api_url(&host.api_url(true), true),
                Some(host.clone())
            );
        }
        let host = Host::parse("example.com").unwrap();
        assert_eq!(host.api_url(true), "https://example.com:7000");
        assert_eq!(
            host.client_ws_url(true),
            "wss://example.com:7000/ws/client"
        );
        assert_eq!(
            host.federation_ws_url(false),
            "ws://example.com:7000/ws/federation"
        );
    }

    #[test]
    fn only_exact_api_urls_name_hosts() {
        for url in [
            "https://example.com",
            "https://Example.com:7000",
            "https://example.com:7000/",
            "http://example.com:7000",
            "https://example.com:07000",
            "https://https://example.com:7000",
            "https://example.com:7000@evil.com",
            "https://evil.com:7000/path",
        ] {
            assert_eq!(Host::from_api_url(url, true), None, "{url}");
        }
    }

    #[test]
    fn hosts_deserialize_canonically() {
        let host: Host = serde_json::from_str(r#""Example.COM:7000""#).unwrap();
        assert_eq!(host, Host::parse("example.com").unwrap());
        assert_eq!(serde_json::to_string(&host).unwrap(), r#""example.com""#);
        assert!(serde_json::from_str::<Host>(r#""example.com/x""#).is_err());
    }
}
