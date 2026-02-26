use thiserror::Error;
use uuid::Uuid;

use runelink_types::ws::WsError;

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

pub type FederationRequestResult<T> = Result<T, FederationRequestError>;
