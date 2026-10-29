use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod versions;

pub const HTTP_CAPABILITY_HEADER: &str = "runelink-capability";

macro_rules! define_capabilities {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        #[derive(Clone, Debug)]
        pub enum Capability {
            $($variant,)+
            Unknown(String),
        }

        impl Capability {
            pub const KNOWN: &'static [Self] = &[$(Self::$variant,)+];

            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $id,)+
                    Self::Unknown(id) => id,
                }
            }

            pub fn with_version(
                &self,
                version: CapabilityVersion,
            ) -> VersionedCapability {
                VersionedCapability {
                    capability: self.clone(),
                    version,
                }
            }
        }

        impl From<String> for Capability {
            fn from(id: String) -> Self {
                match id.as_str() {
                    $($id => Self::$variant,)+
                    _ => Self::Unknown(id),
                }
            }
        }

        impl From<&str> for Capability {
            fn from(id: &str) -> Self {
                Self::from(id.to_owned())
            }
        }
    };
}

define_capabilities! {
    AuthAuthenticate => "auth.authenticate",
    AuthDiscovery => "auth.discovery",
    AuthRegisterClient => "auth.register-client",
    AuthSignup => "auth.signup",
    AuthToken => "auth.token",
    AuthUserinfo => "auth.userinfo",
    ChannelsUpdate => "channels.update",
    ChannelsCreate => "channels.create",
    ChannelsDelete => "channels.delete",
    ChannelsEvents => "channels.events",
    ChannelsMove => "channels.move",
    ChannelsRead => "channels.read",
    ConnectionState => "connection.state",
    MembershipsEvents => "memberships.events",
    MembershipsRead => "memberships.read",
    MembershipsWrite => "memberships.write",
    MessagesUpdate => "messages.update",
    MessagesCreate => "messages.create",
    MessagesDelete => "messages.delete",
    MessagesEvents => "messages.events",
    MessagesRead => "messages.read",
    Ping => "ping",
    ServersUpdate => "servers.update",
    ServersCreate => "servers.create",
    ServersDelete => "servers.delete",
    ServersEvents => "servers.events",
    ServersRead => "servers.read",
    UsersCreate => "users.create",
    UsersDelete => "users.delete",
    UsersEvents => "users.events",
    UsersRead => "users.read",
}

impl PartialEq for Capability {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Capability {}

impl PartialOrd for Capability {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Capability {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for Capability {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A CalVer capability version: `YYYY.MM.DD`, with an optional `.N` revision
/// for additional versions on the same day. Versions are labels, so dates are
/// not checked against the calendar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityVersion {
    year: u16,
    month: u16,
    day: u16,
    revision: u16,
}

impl CapabilityVersion {
    pub const fn new(year: u16, month: u16, day: u16) -> Self {
        assert!(
            has_valid_date_fields(year, month, day),
            "invalid capability version"
        );
        Self {
            year,
            month,
            day,
            revision: 0,
        }
    }

    pub const fn with_revision(self, revision: u16) -> Self {
        Self { revision, ..self }
    }
}

const fn has_valid_date_fields(year: u16, month: u16, day: u16) -> bool {
    year <= 9999 && matches!(month, 1..=12) && matches!(day, 1..=31)
}

impl fmt::Display for CapabilityVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}.{:02}.{:02}",
            self.year, self.month, self.day
        )?;
        if self.revision > 0 {
            write!(formatter, ".{}", self.revision)?;
        }
        Ok(())
    }
}

impl fmt::Debug for CapabilityVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidCapabilityVersion;

impl fmt::Display for InvalidCapabilityVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid capability version")
    }
}

impl std::error::Error for InvalidCapabilityVersion {}

impl FromStr for CapabilityVersion {
    type Err = InvalidCapabilityVersion;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let fields: Vec<u16> = value
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| InvalidCapabilityVersion)?;
        let (year, month, day, revision) = match fields[..] {
            [year, month, day] => (year, month, day, 0),
            [year, month, day, revision] => (year, month, day, revision),
            _ => return Err(InvalidCapabilityVersion),
        };
        let version = Self {
            year,
            month,
            day,
            revision,
        };
        // Display defines the one canonical spelling, which rules out signs,
        // missing padding, leading zeros, and `.0`.
        if !has_valid_date_fields(year, month, day)
            || version.to_string() != value
        {
            return Err(InvalidCapabilityVersion);
        }
        Ok(version)
    }
}

impl Serialize for CapabilityVersion {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CapabilityVersion {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedCapability {
    pub capability: Capability,
    pub version: CapabilityVersion,
}

impl fmt::Display for VersionedCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.capability, self.version)
    }
}

pub type CapabilityCatalog = fn(&Capability) -> &'static [CapabilityVersion];
pub type SupportedCapabilities = BTreeMap<Capability, Vec<CapabilityVersion>>;
pub type NegotiatedCapabilities = BTreeMap<Capability, CapabilityVersion>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityHello {
    pub negotiation_version: u16,
    pub capabilities: SupportedCapabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityWelcome {
    pub negotiation_version: u16,
    pub capabilities: NegotiatedCapabilities,
    pub server_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityDiscovery {
    pub negotiation_version: u16,
    pub server_version: String,
    pub http: SupportedCapabilities,
    pub client_websocket: SupportedCapabilities,
    pub federation_websocket: SupportedCapabilities,
}

pub fn supported_capabilities(
    catalog: CapabilityCatalog,
) -> SupportedCapabilities {
    Capability::KNOWN
        .iter()
        .filter_map(|capability| {
            let versions = catalog(capability);
            (!versions.is_empty())
                .then(|| (capability.clone(), versions.to_vec()))
        })
        .collect()
}

pub fn preferred_version(
    catalog: CapabilityCatalog,
    capability: &Capability,
) -> Option<CapabilityVersion> {
    catalog(capability).iter().copied().max()
}

pub fn catalog_supports(
    catalog: CapabilityCatalog,
    capability: &VersionedCapability,
) -> bool {
    catalog(&capability.capability).contains(&capability.version)
}

pub fn negotiate_capabilities(
    offered: &SupportedCapabilities,
    supported: &SupportedCapabilities,
) -> NegotiatedCapabilities {
    offered
        .iter()
        .filter_map(|(id, offered_versions)| {
            let supported_versions = supported.get(id)?;
            offered_versions
                .iter()
                .filter(|version| supported_versions.contains(version))
                .max()
                .copied()
                .map(|version| (id.clone(), version))
        })
        .collect()
}

pub fn supports(
    capabilities: &NegotiatedCapabilities,
    capability: &VersionedCapability,
) -> bool {
    capabilities.get(&capability.capability) == Some(&capability.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: CapabilityVersion = CapabilityVersion::new(2026, 10, 16);
    const V2: CapabilityVersion = V1.with_revision(1);
    const V3: CapabilityVersion = CapabilityVersion::new(2026, 10, 17);
    const V4: CapabilityVersion = CapabilityVersion::new(2026, 11, 1);

    fn messages_read_only(
        capability: &Capability,
    ) -> &'static [CapabilityVersion] {
        if capability == &Capability::MessagesRead {
            &[V1, V2]
        } else {
            &[]
        }
    }

    #[test]
    fn versions_order_by_date_then_revision() {
        let v10 = V1.with_revision(10);
        let v9 = V1.with_revision(9);
        assert!(V1 < V2 && V2 < v9 && v9 < v10 && v10 < V3 && V3 < V4);
    }

    #[test]
    fn versions_round_trip_through_their_canonical_spelling() {
        for spelling in [
            "2026.10.16",
            "2026.10.16.1",
            "2026.01.05.12",
            "2026.10.16.65535",
        ] {
            let version: CapabilityVersion = spelling.parse().unwrap();
            assert_eq!(version.to_string(), spelling);
        }
        assert_eq!(serde_json::to_string(&V2).unwrap(), r#""2026.10.16.1""#);
        assert!(serde_json::from_str::<CapabilityVersion>("1").is_err());
    }

    #[test]
    fn non_canonical_versions_are_rejected() {
        for spelling in [
            "",
            "1",
            "2026.10",
            "2026.1.16",
            "2026.10.6",
            "26.10.16",
            "2026.10.16.0",
            "2026.10.16.01",
            "2026.10.16.",
            "2026.10.16.1.1",
            "2026.13.16",
            "2026.10.32",
            "2026.00.16",
            "2026.10.+1",
            "2026.10.16.-1",
            "2026.10.16.65536",
            "2026.10.16.99999",
            "2026..16",
            " 2026.10.16",
            "2026.10.16 ",
            "２026.10.16",
        ] {
            assert!(
                spelling.parse::<CapabilityVersion>().is_err(),
                "{spelling} should be rejected"
            );
        }
    }

    #[test]
    fn negotiation_selects_highest_common_version_per_capability() {
        let offered = BTreeMap::from([
            ("messages.read".into(), vec![V1, V2, V4]),
            ("messages.create".into(), vec![V1]),
        ]);
        let supported = BTreeMap::from([
            ("messages.read".into(), vec![V1, V2, V3]),
            ("messages.delete".into(), vec![V1]),
        ]);

        assert_eq!(
            negotiate_capabilities(&offered, &supported),
            BTreeMap::from([("messages.read".into(), V2)])
        );
    }

    #[test]
    fn versioned_capability_has_wire_format() {
        assert_eq!(
            Capability::MessagesRead.with_version(V2).to_string(),
            "messages.read@2026.10.16.1"
        );
        assert_eq!(Capability::MessagesRead.to_string(), "messages.read");
    }

    #[test]
    fn unknown_capability_round_trips_as_a_map_key() {
        let json = r#"{"messages.read":["2026.10.16"],"vendor.search":["2026.10.17"]}"#;
        let capabilities: SupportedCapabilities =
            serde_json::from_str(json).unwrap();

        assert_eq!(
            capabilities.get(&Capability::MessagesRead),
            Some(&vec![V1])
        );
        assert_eq!(
            capabilities.get(&Capability::Unknown("vendor.search".into())),
            Some(&vec![V3])
        );
        assert_eq!(serde_json::to_string(&capabilities).unwrap(), json);
    }

    #[test]
    fn catalog_omits_unsupported_capabilities() {
        assert_eq!(
            supported_capabilities(messages_read_only),
            BTreeMap::from([(Capability::MessagesRead, vec![V1, V2])])
        );
        assert_eq!(
            preferred_version(messages_read_only, &Capability::MessagesRead),
            Some(V2)
        );
        assert_eq!(
            preferred_version(messages_read_only, &Capability::MessagesCreate),
            None
        );
        assert_eq!(
            preferred_version(
                messages_read_only,
                &Capability::Unknown("vendor.search".into())
            ),
            None
        );
    }
}
