use std::{
    io,
    net::{IpAddr, SocketAddr},
};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use runelink_types::Host;
use tokio::net::{ToSocketAddrs, lookup_host};

/// Which addresses this host connects to federation peers at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerPolicy {
    /// Any address, for a host that is itself private, such as in local
    /// development.
    AnyAddress,
    /// Only public addresses, so that peers named in tokens, memberships, and
    /// updates can't make this host reach its own private network.
    PublicOnly,
}

impl PeerPolicy {
    /// The policy for a host with this public host: private peers are allowed
    /// only if it is itself a private address, or resolves to one.
    pub async fn for_public_host(public_host: &Host) -> Self {
        match lookup_host(public_host.authority()).await {
            Ok(mut addrs) => {
                if addrs.any(|addr| !is_public(addr.ip())) {
                    Self::AnyAddress
                } else {
                    Self::PublicOnly
                }
            }
            Err(error) => {
                log::warn!(
                    "Failed resolving {public_host}, so only public peers \
                     are allowed: {error}"
                );
                Self::PublicOnly
            }
        }
    }

    pub fn allows(self, ip: IpAddr) -> bool {
        self == Self::AnyAddress || is_public(ip)
    }

    /// Whether `host` is allowed, if it is a literal IP address. Those aren't
    /// resolved, so the HTTP client's resolver never sees them.
    pub fn allows_literal(self, host: &Host) -> bool {
        match host.authority().parse::<SocketAddr>() {
            Ok(addr) => self.allows(addr.ip()),
            Err(_) => true,
        }
    }

    /// Resolves `host` to the addresses this policy allows connecting to.
    pub async fn resolve(self, host: &Host) -> io::Result<Vec<SocketAddr>> {
        self.lookup(host.authority()).await
    }

    async fn lookup(
        self,
        host: impl ToSocketAddrs,
    ) -> io::Result<Vec<SocketAddr>> {
        let addrs: Vec<_> = lookup_host(host)
            .await?
            .filter(|addr| self.allows(addr.ip()))
            .collect();
        if addrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host has no addresses that federation may connect to",
            ));
        }
        Ok(addrs)
    }
}

/// Resolves the HTTP client's host names, so its connections are checked
/// against the addresses it actually connects to.
impl Resolve for PeerPolicy {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = *self;
        Box::pin(async move {
            // The client sets the port.
            let addrs = policy.lookup((name.as_str(), 0)).await?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Whether an address is publicly routable, rather than loopback, private,
/// link-local, or otherwise reserved for a local network.
fn is_public(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            let [first, second, ..] = ip.octets();
            let this_network = first == 0;
            let shared = first == 100 && second & 0b1100_0000 == 64;
            !(this_network
                || shared
                || ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local())
        }
        IpAddr::V6(ip) => {
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

#[cfg(test)]
mod tests {
    use runelink_types::Host;

    use super::PeerPolicy;

    #[test]
    fn public_only_refuses_local_addresses() {
        for (ip, public) in [
            ("93.184.215.14", true),
            ("100.63.255.255", true),
            ("100.128.0.0", true),
            ("172.32.0.1", true),
            ("2606:4700::1", true),
            ("0.0.0.0", false),
            ("0.1.2.3", false),
            ("127.0.0.1", false),
            ("10.0.0.1", false),
            ("172.16.0.1", false),
            ("192.168.1.1", false),
            ("169.254.169.254", false),
            ("100.64.0.1", false),
            ("::", false),
            ("::1", false),
            ("fc00::1", false),
            ("fd00:ec2::254", false),
            ("fe80::1", false),
            ("::ffff:127.0.0.1", false),
            ("::ffff:169.254.169.254", false),
        ] {
            let ip = ip.parse().unwrap();
            assert_eq!(PeerPolicy::PublicOnly.allows(ip), public, "{ip}");
            assert!(PeerPolicy::AnyAddress.allows(ip), "{ip}");
        }
    }

    #[test]
    fn literal_hosts_are_checked() {
        for (host, allowed) in [
            ("runelink.chat", true),
            ("localhost", true),
            ("93.184.215.14:7001", true),
            ("127.0.0.1", false),
            ("[::1]:7001", false),
        ] {
            let host = Host::parse(host).unwrap();
            assert_eq!(
                PeerPolicy::PublicOnly.allows_literal(&host),
                allowed,
                "{host}"
            );
        }
    }

    #[tokio::test]
    async fn private_public_hosts_allow_private_peers() {
        for host in ["localhost:7001", "127.0.0.1", "[::1]"] {
            let host = Host::parse(host).unwrap();
            assert_eq!(
                PeerPolicy::for_public_host(&host).await,
                PeerPolicy::AnyAddress,
                "{host}"
            );
        }
    }
}
