use runelink_types::capability::{
    Capability, CapabilityVersion, preferred_version,
    versions::{auth, channels, memberships, messages, servers, system, users},
};

use crate::error::{Error, Result};

pub const fn http(capability: &Capability) -> &'static [CapabilityVersion] {
    match capability {
        Capability::AuthAuthenticate => &[],
        Capability::AuthDiscovery => &[],
        Capability::AuthRegisterClient => &[],
        Capability::AuthSignup => auth::SIGNUP_VERSIONS,
        Capability::AuthToken => auth::TOKEN_VERSIONS,
        Capability::AuthUserinfo => &[],
        Capability::ChannelsUpdate => channels::UPDATE_VERSIONS,
        Capability::ChannelsCreate => channels::CREATE_VERSIONS,
        Capability::ChannelsDelete => channels::DELETE_VERSIONS,
        Capability::ChannelsEvents => &[],
        Capability::ChannelsMove => channels::MOVE_VERSIONS,
        Capability::ChannelsRead => channels::READ_VERSIONS,
        Capability::ConnectionState => &[],
        Capability::MembershipsEvents => &[],
        Capability::MembershipsRead => memberships::READ_VERSIONS,
        Capability::MembershipsWrite => memberships::WRITE_VERSIONS,
        Capability::MessagesUpdate => messages::UPDATE_VERSIONS,
        Capability::MessagesCreate => messages::CREATE_VERSIONS,
        Capability::MessagesDelete => messages::DELETE_VERSIONS,
        Capability::MessagesEvents => &[],
        Capability::MessagesRead => messages::READ_VERSIONS,
        Capability::Ping => &[],
        Capability::ServersUpdate => servers::UPDATE_VERSIONS,
        Capability::ServersCreate => servers::CREATE_VERSIONS,
        Capability::ServersDelete => servers::DELETE_VERSIONS,
        Capability::ServersEvents => &[],
        Capability::ServersRead => servers::READ_VERSIONS,
        Capability::UsersCreate => users::CREATE_VERSIONS,
        Capability::UsersDelete => users::DELETE_VERSIONS,
        Capability::UsersEvents => &[],
        Capability::UsersRead => users::READ_VERSIONS,
        Capability::Unknown(_) => &[],
    }
}

/// The client websocket capability versions this client implements.
pub const fn client_websocket(
    capability: &Capability,
) -> &'static [CapabilityVersion] {
    match capability {
        Capability::AuthAuthenticate => auth::AUTHENTICATE_VERSIONS,
        Capability::AuthDiscovery => auth::DISCOVERY_VERSIONS,
        Capability::AuthRegisterClient => &[],
        Capability::AuthSignup => auth::SIGNUP_VERSIONS,
        Capability::AuthToken => auth::TOKEN_VERSIONS,
        Capability::AuthUserinfo => &[],
        Capability::ChannelsUpdate => channels::UPDATE_VERSIONS,
        Capability::ChannelsCreate => channels::CREATE_VERSIONS,
        Capability::ChannelsDelete => channels::DELETE_VERSIONS,
        Capability::ChannelsEvents => channels::EVENTS_VERSIONS,
        Capability::ChannelsMove => channels::MOVE_VERSIONS,
        Capability::ChannelsRead => channels::READ_VERSIONS,
        Capability::ConnectionState => system::CONNECTION_STATE_VERSIONS,
        Capability::MembershipsEvents => memberships::EVENTS_VERSIONS,
        Capability::MembershipsRead => memberships::READ_VERSIONS,
        Capability::MembershipsWrite => memberships::WRITE_VERSIONS,
        Capability::MessagesUpdate => messages::UPDATE_VERSIONS,
        Capability::MessagesCreate => messages::CREATE_VERSIONS,
        Capability::MessagesDelete => messages::DELETE_VERSIONS,
        Capability::MessagesEvents => messages::EVENTS_VERSIONS,
        Capability::MessagesRead => messages::READ_VERSIONS,
        Capability::Ping => system::PING_VERSIONS,
        Capability::ServersUpdate => servers::UPDATE_VERSIONS,
        Capability::ServersCreate => servers::CREATE_VERSIONS,
        Capability::ServersDelete => servers::DELETE_VERSIONS,
        Capability::ServersEvents => servers::EVENTS_VERSIONS,
        Capability::ServersRead => servers::READ_VERSIONS,
        Capability::UsersCreate => users::CREATE_VERSIONS,
        Capability::UsersDelete => users::DELETE_VERSIONS,
        Capability::UsersEvents => users::EVENTS_VERSIONS,
        Capability::UsersRead => users::READ_VERSIONS,
        Capability::Unknown(_) => &[],
    }
}

pub fn preferred_client_websocket_version(
    capability: &Capability,
) -> Option<CapabilityVersion> {
    preferred_version(client_websocket, capability)
}

pub fn preferred_http_version(
    capability: &Capability,
) -> Option<CapabilityVersion> {
    preferred_version(http, capability)
}

pub(crate) fn http_header_value(capability: &Capability) -> Result<String> {
    preferred_http_version(capability)
        .map(|version| capability.with_version(version).to_string())
        .ok_or_else(|| Error::UnsupportedCapability(capability.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_capability_has_no_preferred_version() {
        assert_eq!(preferred_http_version(&Capability::Ping), None);
        assert_eq!(
            preferred_http_version(&Capability::Unknown("custom.read".into())),
            None
        );
        assert_eq!(
            http_header_value(&Capability::Ping)
                .unwrap_err()
                .to_string(),
            "This client does not support the ping capability"
        );
    }
}
