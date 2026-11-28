use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use runelink_client::Error as ClientError;
use runelink_types::ws::WsError;
use serde::Serialize;
use thiserror::Error;
use tokio::task::JoinError;

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Database connection error: {0}")]
    DbConnectionError(String),

    #[error("Unique constraint violation")]
    UniqueViolation,

    #[error("A referenced resource no longer exists")]
    ForeignKeyViolation,

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Resource not found")]
    NotFound,

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Unauthorized: {0}")]
    AuthError(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Upstream error: {0}")]
    Client(#[from] ClientError),
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
                ApiError::DbConnectionError(e.to_string())
            }

            sqlx::Error::Database(db_err) => match db_err.code().as_deref() {
                Some("23505") => ApiError::UniqueViolation,
                // A concurrent change removed a referenced row
                Some("23503") => {
                    log::warn!("Foreign key violation: {}", db_err.message());
                    ApiError::ForeignKeyViolation
                }
                _ => ApiError::DatabaseError(db_err.message().to_string()),
            },

            sqlx::Error::RowNotFound => ApiError::NotFound,

            _ => ApiError::Unknown(e.to_string()),
        }
    }
}

impl From<JoinError> for ApiError {
    fn from(e: JoinError) -> Self {
        ApiError::Unknown(format!("Join error: {e}"))
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::DbConnectionError(_)
            | ApiError::DatabaseError(_)
            | ApiError::Internal(_)
            | ApiError::Unknown(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Conflict(_)
            | ApiError::UniqueViolation
            | ApiError::ForeignKeyViolation => StatusCode::CONFLICT,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::AuthError(_) => StatusCode::UNAUTHORIZED,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Client(ref client_err) => match client_err {
                ClientError::Status(code, _) => *code,
                _ => StatusCode::BAD_GATEWAY,
            },
        };
        let body = Json(ErrorResponse {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}

impl From<ApiError> for WsError {
    fn from(error: ApiError) -> Self {
        let code = match error {
            ApiError::AuthError(_) => "auth_error",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::NotFound => "not_found",
            ApiError::Conflict(_)
            | ApiError::UniqueViolation
            | ApiError::ForeignKeyViolation => "conflict",
            ApiError::DbConnectionError(_)
            | ApiError::DatabaseError(_)
            | ApiError::Internal(_)
            | ApiError::Unknown(_)
            | ApiError::Client(_) => "internal_error",
        };
        // The code already names the kind of error, which a peer adds back
        // when it passes the error on.
        let message = match error {
            ApiError::AuthError(message)
            | ApiError::BadRequest(message)
            | ApiError::Conflict(message) => message,
            error => error.to_string(),
        };
        WsError {
            code: code.to_string(),
            message,
            details: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use runelink_types::{Host, ws::WsError};

    use super::ApiError;
    use crate::ws::error::FederationRequestError;

    #[test]
    fn errors_passed_between_hosts_keep_their_message() {
        let host = Host::parse("peer.example").unwrap();
        for error in [
            ApiError::AuthError("x".into()),
            ApiError::BadRequest("x".into()),
            ApiError::Conflict("x".into()),
        ] {
            let sent = WsError::from(error);
            let code = sent.code.clone();
            let received = FederationRequestError::Remote {
                code: sent.code.clone(),
                message: sent.message.clone(),
                error: Box::new(sent),
            }
            .into_api_error(&host);
            let resent = WsError::from(received);
            assert_eq!((resent.code, resent.message), (code, "x".into()));
        }
    }
}
