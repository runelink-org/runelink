use std::{error::Error, fmt};

pub const MAX_USERNAME_LENGTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    UsernameEmpty,
    UsernameTooLong,
    HostEmpty,
    HostInvalidCharacters,
    HostMultiplePorts,
    HostPortNotAllowed,
    HostInvalidPort,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsernameEmpty => write!(f, "Username cannot be empty."),
            Self::UsernameTooLong => write!(
                f,
                "Username cannot be longer than {MAX_USERNAME_LENGTH} characters."
            ),
            Self::HostEmpty => write!(f, "Host cannot be empty."),
            Self::HostInvalidCharacters => write!(
                f,
                "Host may only contain lowercase letters, digits, dots, hyphens, and an optional :port.",
            ),
            Self::HostMultiplePorts => {
                write!(f, "Host can include at most one port separator.")
            }
            Self::HostPortNotAllowed => {
                write!(f, "Host must not include a port here.")
            }
            Self::HostInvalidPort => {
                write!(f, "Host port must contain digits only.")
            }
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

pub fn normalize_host_input(input: &str) -> String {
    let lowercased = input.trim().to_ascii_lowercase();
    lowercased
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string()
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

pub fn validate_host(input: &str) -> Result<String, ValidationError> {
    validate_host_internal(input, true)
}

pub fn validate_config_host(input: &str) -> Result<String, ValidationError> {
    validate_host_internal(input, false)
}

fn validate_host_internal(
    input: &str,
    allow_port: bool,
) -> Result<String, ValidationError> {
    let normalized = normalize_host_input(input);

    if normalized.is_empty() {
        return Err(ValidationError::HostEmpty);
    }

    let colon_count = normalized.chars().filter(|ch| *ch == ':').count();
    if colon_count > 1 {
        return Err(ValidationError::HostMultiplePorts);
    }

    let (host_part, port_part) = match normalized.split_once(':') {
        Some((host_part, port_part)) => {
            if !allow_port {
                return Err(ValidationError::HostPortNotAllowed);
            }
            (host_part, Some(port_part))
        }
        None => (normalized.as_str(), None),
    };

    if host_part.is_empty() {
        return Err(ValidationError::HostEmpty);
    }

    if !host_part.chars().all(|ch| {
        ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-'
    }) {
        return Err(ValidationError::HostInvalidCharacters);
    }

    if let Some(port_part) = port_part {
        if port_part.is_empty()
            || !port_part.chars().all(|ch| ch.is_ascii_digit())
        {
            return Err(ValidationError::HostInvalidPort);
        }
    }

    Ok(normalized)
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
    fn normalizes_and_validates_hosts() {
        assert_eq!(
            validate_host(" HTTPS://Example.COM:7000/ ").unwrap(),
            "example.com:7000"
        );
        assert_eq!(validate_config_host("Example.COM").unwrap(), "example.com");
    }

    #[test]
    fn rejects_invalid_ports() {
        assert_eq!(
            validate_host("example.com:abc").unwrap_err(),
            ValidationError::HostInvalidPort
        );
        assert_eq!(
            validate_host("example.com:7000:1").unwrap_err(),
            ValidationError::HostMultiplePorts
        );
    }

    #[test]
    fn rejects_ports_in_config_hosts() {
        assert_eq!(
            validate_config_host("example.com:7000").unwrap_err(),
            ValidationError::HostPortNotAllowed
        );
    }

    #[test]
    fn rejects_invalid_host_characters() {
        assert_eq!(
            validate_host("exa$mple.com").unwrap_err(),
            ValidationError::HostInvalidCharacters
        );
    }
}
