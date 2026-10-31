use reqwest::StatusCode;
use runelink_types::capability::Capability;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("unexpected status {0}: {1}")]
    Status(StatusCode, String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("This client does not support the {0} capability")]
    UnsupportedCapability(Capability),
}

impl Error {
    pub(crate) fn from_status(status: StatusCode, body: String) -> Self {
        #[derive(serde::Deserialize)]
        struct CapabilityError {
            code: String,
            message: String,
            capability: Capability,
            supported_versions:
                Vec<runelink_types::capability::CapabilityVersion>,
        }
        if let Ok(error) = serde_json::from_str::<CapabilityError>(&body)
            && error.code == "unsupported_capability"
        {
            return Self::Status(
                status,
                format!(
                    "{}; {} host versions: {:?}",
                    error.message, error.capability, error.supported_versions
                ),
            );
        }
        Self::Status(status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_errors_are_readable_and_legacy_errors_are_preserved() {
        let body = r#"{"code":"unsupported_capability","message":"host needs updating","capability":"messages.read","supported_versions":["2026.10.16"]}"#;
        let error = Error::from_status(StatusCode::BAD_REQUEST, body.into());
        assert!(error.to_string().contains("host needs updating"));
        assert!(
            error
                .to_string()
                .contains("messages.read host versions: [2026.10.16]")
        );
        for body in [
            "legacy error",
            r#"{"code":"unsupported_capability","message":"old"}"#,
        ] {
            let Error::Status(_, message) =
                Error::from_status(StatusCode::BAD_REQUEST, body.into())
            else {
                panic!("expected status error")
            };
            assert_eq!(message, body);
        }
    }
}
