use runelink_types::capability::{
    Capability, CapabilityCatalog, CapabilityVersion, NegotiatedCapabilities,
    SupportedCapabilities,
    versions::{auth, channels, memberships, messages, servers, system, users},
};

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
        Capability::ChannelsCreate => channels::CREATE_VERSIONS,
        Capability::ChannelsDelete => channels::DELETE_VERSIONS,
        Capability::ChannelsEvents => channels::EVENTS_VERSIONS,
        Capability::ChannelsRead => channels::READ_VERSIONS,
        Capability::ConnectionState => system::CONNECTION_STATE_VERSIONS,
        Capability::MembershipsEvents => memberships::EVENTS_VERSIONS,
        Capability::MembershipsRead => memberships::READ_VERSIONS,
        Capability::MembershipsWrite => memberships::WRITE_VERSIONS,
        Capability::MessagesCreate => messages::CREATE_VERSIONS,
        Capability::MessagesDelete => messages::DELETE_VERSIONS,
        Capability::MessagesEvents => messages::EVENTS_VERSIONS,
        Capability::MessagesRead => messages::READ_VERSIONS,
        Capability::Ping => system::PING_VERSIONS,
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

pub const fn federation_websocket(
    capability: &Capability,
) -> &'static [CapabilityVersion] {
    match capability {
        Capability::AuthAuthenticate => &[],
        Capability::AuthDiscovery => &[],
        Capability::AuthRegisterClient => &[],
        Capability::AuthSignup => &[],
        Capability::AuthToken => &[],
        Capability::AuthUserinfo => &[],
        Capability::ChannelsCreate => channels::CREATE_VERSIONS,
        Capability::ChannelsDelete => channels::DELETE_VERSIONS,
        Capability::ChannelsEvents => channels::EVENTS_VERSIONS,
        Capability::ChannelsRead => channels::READ_VERSIONS,
        Capability::ConnectionState => system::CONNECTION_STATE_VERSIONS,
        Capability::MembershipsEvents => memberships::EVENTS_VERSIONS,
        Capability::MembershipsRead => memberships::READ_VERSIONS,
        Capability::MembershipsWrite => memberships::WRITE_VERSIONS,
        Capability::MessagesCreate => messages::CREATE_VERSIONS,
        Capability::MessagesDelete => messages::DELETE_VERSIONS,
        Capability::MessagesEvents => messages::EVENTS_VERSIONS,
        Capability::MessagesRead => messages::READ_VERSIONS,
        Capability::Ping => &[],
        Capability::ServersCreate => servers::CREATE_VERSIONS,
        Capability::ServersDelete => servers::DELETE_VERSIONS,
        Capability::ServersEvents => servers::EVENTS_VERSIONS,
        Capability::ServersRead => servers::READ_VERSIONS,
        Capability::UsersCreate => &[],
        Capability::UsersDelete => users::DELETE_VERSIONS,
        Capability::UsersEvents => users::EVENTS_VERSIONS,
        Capability::UsersRead => users::READ_VERSIONS,
        Capability::Unknown(_) => &[],
    }
}

pub const fn http(capability: &Capability) -> &'static [CapabilityVersion] {
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

const MAX_LOGGED_VERSIONS: usize = 8;

/// Logs offered capabilities this host supports, but at no common version.
pub fn log_version_mismatches(
    peer: &str,
    offered: &SupportedCapabilities,
    negotiated: &NegotiatedCapabilities,
    catalog: CapabilityCatalog,
) {
    for (capability, versions) in offered {
        let supported = catalog(capability);
        if !supported.is_empty()
            && !versions.is_empty()
            && !negotiated.contains_key(capability)
        {
            log::warn!(
                "{peer} offered {capability} versions {}, but this host supports versions {supported:?}",
                format_versions(versions)
            );
        }
    }
}

/// Formats untrusted peer version lists without unbounded log lines.
fn format_versions(versions: &[CapabilityVersion]) -> String {
    let shown = &versions[..versions.len().min(MAX_LOGGED_VERSIONS)];
    match versions.len() - shown.len() {
        0 => format!("{shown:?}"),
        hidden => format!("{shown:?} (+{hidden} more)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_version_lists() {
        let versions: Vec<_> = (0..10)
            .map(|revision| {
                CapabilityVersion::new(2026, 10, 16).with_revision(revision)
            })
            .collect();
        assert_eq!(
            format_versions(&versions[..2]),
            "[2026.10.16, 2026.10.16.1]"
        );
        assert_eq!(
            format_versions(&versions),
            "[2026.10.16, 2026.10.16.1, 2026.10.16.2, 2026.10.16.3, \
             2026.10.16.4, 2026.10.16.5, 2026.10.16.6, 2026.10.16.7] (+2 more)"
        );
    }
}
