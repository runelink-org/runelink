pub fn pad_host(host: &str) -> String {
    if host.starts_with('[') {
        // IPv6 literal
        match host.find(']') {
            Some(closing) => {
                let after = &host[closing + 1..];
                if after.starts_with(':') {
                    host.to_string()
                } else {
                    format!("{host}:7000")
                }
            }
            None => {
                // malformed IPv6, just append
                format!("{host}:7000")
            }
        }
    } else if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:7000")
    }
}

pub fn strip_default_port(host: &str) -> String {
    match host.strip_suffix(":7000") {
        Some(stripped) => stripped.to_string(),
        None => host.to_string(),
    }
}

pub fn host_from_issuer(issuer: &str) -> String {
    let host = issuer
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    strip_default_port(host)
}

pub fn get_api_url(host: &str) -> String {
    let host_with_port = pad_host(host);
    format!("http://{host_with_port}")
}

pub fn get_client_ws_url(host: &str) -> String {
    let host_with_port = pad_host(host);
    format!("ws://{host_with_port}/ws/client")
}

pub fn get_federation_ws_url(host: &str) -> String {
    let host_with_port = pad_host(host);
    format!("ws://{host_with_port}/ws/federation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_default_port_hostname() {
        assert_eq!(strip_default_port("example.com"), "example.com");
        assert_eq!(strip_default_port("example.com:7000"), "example.com");
    }

    #[test]
    fn test_strip_default_port_non_default_port() {
        assert_eq!(strip_default_port("example.com:8080"), "example.com:8080");
    }

    #[test]
    fn test_strip_default_port_ipv6() {
        assert_eq!(strip_default_port("[::1]"), "[::1]");
        assert_eq!(strip_default_port("[::1]:7000"), "[::1]");
        assert_eq!(strip_default_port("[::1]:4321"), "[::1]:4321");
    }

    #[test]
    fn test_host_from_issuer_strips_default_port() {
        assert_eq!(host_from_issuer("http://example.com:7000"), "example.com");
        assert_eq!(host_from_issuer("https://example.com/"), "example.com");
    }

    #[test]
    fn test_no_port() {
        let url = get_api_url("example.com");
        assert_eq!(url, "http://example.com:7000");
    }

    #[test]
    fn test_with_port() {
        let url = get_api_url("example.com:8080");
        assert_eq!(url, "http://example.com:8080");
    }

    #[test]
    fn test_ipv6_no_port() {
        let url = get_api_url("[::1]");
        assert_eq!(url, "http://[::1]:7000");
    }

    #[test]
    fn test_ipv6_with_port() {
        let url = get_api_url("[::1]:4321");
        assert_eq!(url, "http://[::1]:4321");
    }

    #[test]
    fn test_malformed_ipv6() {
        // no closing ']', treated as no port
        let url = get_api_url("[::1");
        assert_eq!(url, "http://[::1:7000");
    }
}
