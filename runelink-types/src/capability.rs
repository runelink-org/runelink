use std::{
    cmp::Ordering,
    collections::BTreeMap,
    hash::{Hash, Hasher},
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

            pub fn with_version(&self, version: u16) -> VersionedCapability {
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
    ChannelsCreate => "channels.create",
    ChannelsDelete => "channels.delete",
    ChannelsEvents => "channels.events",
    ChannelsRead => "channels.read",
    ConnectionState => "connection.state",
    MembershipsEvents => "memberships.events",
    MembershipsRead => "memberships.read",
    MembershipsWrite => "memberships.write",
    MessagesCreate => "messages.create",
    MessagesDelete => "messages.delete",
    MessagesEvents => "messages.events",
    MessagesRead => "messages.read",
    Ping => "ping",
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

impl std::fmt::Display for Capability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedCapability {
    pub capability: Capability,
    pub version: u16,
}

impl std::fmt::Display for VersionedCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.capability, self.version)
    }
}

pub type CapabilityCatalog = fn(&Capability) -> &'static [u16];
pub type SupportedCapabilities = BTreeMap<Capability, Vec<u16>>;
pub type NegotiatedCapabilities = BTreeMap<Capability, u16>;

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
) -> Option<u16> {
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

    fn messages_read_only(capability: &Capability) -> &'static [u16] {
        if capability == &Capability::MessagesRead {
            &[1, 2]
        } else {
            &[]
        }
    }

    #[test]
    fn negotiation_selects_highest_common_version_per_capability() {
        let offered = BTreeMap::from([
            ("messages.read".into(), vec![1, 2, 4]),
            ("messages.create".into(), vec![1]),
        ]);
        let supported = BTreeMap::from([
            ("messages.read".into(), vec![1, 2, 3]),
            ("messages.delete".into(), vec![1]),
        ]);

        assert_eq!(
            negotiate_capabilities(&offered, &supported),
            BTreeMap::from([("messages.read".into(), 2)])
        );
    }

    #[test]
    fn versioned_capability_has_wire_format() {
        assert_eq!(
            Capability::MessagesRead.with_version(2).to_string(),
            "messages.read@2"
        );
        assert_eq!(Capability::MessagesRead.to_string(), "messages.read");
    }

    #[test]
    fn unknown_capability_round_trips_as_a_map_key() {
        let json = r#"{"messages.read":[1],"vendor.search":[2]}"#;
        let capabilities: SupportedCapabilities =
            serde_json::from_str(json).unwrap();

        assert_eq!(capabilities.get(&Capability::MessagesRead), Some(&vec![1]));
        assert_eq!(
            capabilities.get(&Capability::Unknown("vendor.search".into())),
            Some(&vec![2])
        );
        assert_eq!(serde_json::to_string(&capabilities).unwrap(), json);
    }

    #[test]
    fn catalog_omits_unsupported_capabilities() {
        assert_eq!(
            supported_capabilities(messages_read_only),
            BTreeMap::from([(Capability::MessagesRead, vec![1, 2])])
        );
        assert_eq!(
            preferred_version(messages_read_only, &Capability::MessagesRead),
            Some(2)
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
