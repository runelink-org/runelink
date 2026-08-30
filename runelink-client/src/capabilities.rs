use runelink_types::capability::{
    Capability, preferred_version,
    versions::{auth, channels, memberships, messages, servers, users},
};

use crate::error::{Error, Result};

pub const fn http(capability: &Capability) -> &'static [u16] {
    match capability {
        Capability::AuthAuthenticate => &[],
        Capability::AuthDiscovery => &[],
        Capability::AuthRegisterClient => &[],
        Capability::AuthSignup => auth::SIGNUP_VERSIONS,
        Capability::AuthToken => auth::TOKEN_VERSIONS,
        Capability::AuthUserinfo => &[],
        Capability::ChannelsCreate => channels::CREATE_VERSIONS,
        Capability::ChannelsDelete => channels::DELETE_VERSIONS,
        Capability::ChannelsEvents => &[],
        Capability::ChannelsRead => channels::READ_VERSIONS,
        Capability::ConnectionState => &[],
        Capability::MembershipsEvents => &[],
        Capability::MembershipsRead => memberships::READ_VERSIONS,
        Capability::MembershipsWrite => memberships::WRITE_VERSIONS,
        Capability::MessagesCreate => messages::CREATE_VERSIONS,
        Capability::MessagesDelete => messages::DELETE_VERSIONS,
        Capability::MessagesEvents => &[],
        Capability::MessagesRead => messages::READ_VERSIONS,
        Capability::Ping => &[],
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

pub fn preferred_http_version(capability: &Capability) -> Option<u16> {
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
