use runelink_types::host::{Host, InvalidHost};
use std::{error::Error, fmt};

pub const MAX_USERNAME_LENGTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    UsernameEmpty,
    UsernameTooLong,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsernameEmpty => write!(f, "Username cannot be empty."),
            Self::UsernameTooLong => write!(
                f,
                "Username cannot be longer than {MAX_USERNAME_LENGTH} characters."
            ),
        }
    }
}

impl Error for ValidationError {}

pub fn normalize_username(input: &str) -> String {
    let mut normalized = String::new();
    let mut pending_dash = false;

    for ch in input.trim().chars() {
        let ch = ch.to_ascii_lowercase();

        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if pending_dash && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(ch);
            pending_dash = false;
            continue;
        }

        if ch.is_ascii_whitespace() || ch == '_' || ch == '-' || ch == '.' {
            pending_dash = !normalized.is_empty();
        }
    }

    normalized
}

pub fn validate_username(input: &str) -> Result<String, ValidationError> {
    let normalized = normalize_username(input);
    if normalized.is_empty() {
        return Err(ValidationError::UsernameEmpty);
    }
    if normalized.len() > MAX_USERNAME_LENGTH {
        return Err(ValidationError::UsernameTooLong);
    }
    Ok(normalized)
}

/// Parses a host that a person entered, which may also have a URL scheme or
/// a trailing slash.
pub fn validate_host(input: &str) -> Result<Host, InvalidHost> {
    let input = input.trim().to_ascii_lowercase();
    let host = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(&input);
    Host::parse(host.strip_suffix('/').unwrap_or(host))
        .map_err(|_| InvalidHost(input.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_usernames() {
        assert_eq!(normalize_username("John   Smith"), "john-smith");
        assert_eq!(normalize_username("John_Smith"), "john-smith");
        assert_eq!(normalize_username("John.Smith"), "john-smith");
        assert_eq!(normalize_username("__ John  Smith __"), "john-smith");
        assert_eq!(normalize_username("A@B!"), "ab");
    }

    #[test]
    fn rejects_empty_usernames() {
        assert_eq!(
            validate_username("___!!!").unwrap_err(),
            ValidationError::UsernameEmpty
        );
    }

    #[test]
    fn rejects_long_usernames() {
        assert_eq!(
            validate_username("abcdefghijklmnopqrstuvwxyz-123456").unwrap_err(),
            ValidationError::UsernameTooLong
        );
    }

    #[test]
    fn validates_entered_hosts() {
        for (input, host) in [
            (" HTTPS://Example.COM:7000/ ", "example.com"),
            ("http://localhost:7001", "localhost:7001"),
            ("[::1]:7000", "[::1]"),
        ] {
            assert_eq!(validate_host(input).unwrap().as_str(), host, "{input}");
        }
        for input in
            ["example.com:abc", "example.com:7000:1", "exa$mple.com", ""]
        {
            assert!(validate_host(input).is_err(), "{input}");
        }
    }
}
