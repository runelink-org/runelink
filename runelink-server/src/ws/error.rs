use thiserror::Error;
use uuid::Uuid;

use crate::error::ApiError;
use runelink_types::ws::WsError;

pub type FederationRequestResult<T> = Result<T, FederationRequestError>;

#[derive(Debug, Error)]
pub enum FederationRequestError {
    #[error("No active federation connection for host '{host}'")]
    HostUnavailable { host: String },
    #[error("Timed out waiting for request '{request_id}' reply from '{host}'")]
    Timeout { host: String, request_id: Uuid },
    #[error("Request '{request_id}' waiter dropped before completion")]
    ChannelClosed { request_id: Uuid },
    #[error("Remote federation error [{code}]: {message}")]
    Remote {
        code: String,
        message: String,
        error: WsError,
    },
}

impl FederationRequestError {
    pub fn into_api_error(self, host: &str) -> ApiError {
        match self {
            FederationRequestError::HostUnavailable { .. } => {
                ApiError::Internal(format!(
                    "No active federation websocket connection for host {host}"
                ))
            }
            FederationRequestError::Timeout { .. } => {
                ApiError::Internal(format!(
                    "Timed out waiting for federation websocket reply from {host}"
                ))
            }
            FederationRequestError::ChannelClosed { .. } => {
                ApiError::Internal(format!(
                    "Federation websocket reply channel closed for host {host}"
                ))
            }
            FederationRequestError::Remote { code, message, .. } => {
                match code.as_str() {
                    "auth_error" => ApiError::AuthError(message),
                    "bad_request" => ApiError::BadRequest(message),
                    "not_found" => ApiError::NotFound,
                    "conflict" => ApiError::UniqueViolation,
                    _ => ApiError::Internal(format!(
                        "Remote federation websocket error from {host} [{code}]: {message}"
                    )),
                }
            }
        }
    }
}
