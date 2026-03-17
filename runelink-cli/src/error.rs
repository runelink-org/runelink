use reqwest::StatusCode;
use runelink_client::Error as ClientError;
use std::process::ExitCode;

#[allow(dead_code)]
#[derive(thiserror::Error, Debug)]
pub enum CliError {
    #[error("API request failed: {0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("API returned error status {status}: {message}")]
    ApiStatusError { status: StatusCode, message: String },

    #[error("Failed to deserialize JSON: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid UUID: {0}")]
    UuidError(#[from] uuid::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Invalid Argument: {0}")]
    InvalidArgument(String),

    #[error("Missing Context: {0}")]
    MissingContext(String),

    #[error("Missing Account: Specify an account or set a default account")]
    MissingAccount,

    #[error("No Action Possible: {0}")]
    NoActionPossible(String),

    #[error("Operation Canceled")]
    Cancellation,

    #[error("Unexpected error: {0}")]
    Unknown(String),
}

impl From<ClientError> for CliError {
    fn from(err: ClientError) -> CliError {
        match err {
            ClientError::Reqwest(e) => CliError::ReqwestError(e),
            ClientError::Status(status, message) => {
                CliError::ApiStatusError { status, message }
            }
            ClientError::Json(e) => CliError::JsonError(e),
        }
    }
}

impl CliError {
    pub fn report_for_cli(&self) {
        match self {
            CliError::ReqwestError(e) => {
                if let Some(status) = e.status() {
                    eprintln!("{}: {}", status, e);
                } else {
                    eprintln!("{}", e);
                }
            }
            CliError::InvalidArgument(msg) => eprintln!("{}", msg),
            CliError::NoActionPossible(msg) => eprintln!("{}", msg),
            other_error => eprintln!("{}", other_error),
        }
    }
}
// sysexits.h inspired exit codes
const _EX_OK: u8 = 0;
const EX_USAGE: u8 = 64; // command line usage error
const EX_DATAERR: u8 = 65; // data format error
const EX_NOUSER: u8 = 67; // addressee unknown (not found)
const EX_UNAVAILABLE: u8 = 69; // service unavailable
const EX_SOFTWARE: u8 = 70; // internal software error
const EX_IOERR: u8 = 74; // input/output error
const EX_TEMPFAIL: u8 = 75; // temp failure; user is invited to retry
const EX_PROTOCOL: u8 = 76; // remote error in protocol
const EX_NOPERM: u8 = 77; // permission denied
const EX_CONFIG: u8 = 78; // configuration error
const EX_USER_CANCEL: u8 = 130; // Standard for SIGINT / user interrupt

impl From<CliError> for ExitCode {
    fn from(value: CliError) -> Self {
        ExitCode::from(match value {
            CliError::ReqwestError(e) => {
                if let Some(status) = e.status() {
                    status_to_exit_code(status)
                } else if e.is_timeout() || e.is_connect() {
                    EX_TEMPFAIL
                } else if e.is_request() {
                    EX_USAGE
                } else {
                    EX_PROTOCOL
                }
            }
            CliError::ApiStatusError { status, .. } => {
                status_to_exit_code(status)
            }
            CliError::InvalidArgument(_) => EX_USAGE,
            CliError::JsonError(_) => EX_DATAERR,
            CliError::IoError(_) => EX_IOERR,
            CliError::UuidError(_) => EX_DATAERR,
            CliError::ConfigError(_) => EX_CONFIG,
            CliError::MissingContext(_) => EX_USAGE,
            CliError::MissingAccount => EX_USAGE,
            CliError::NoActionPossible(_) => EX_USAGE,
            CliError::Cancellation => EX_USER_CANCEL,
            CliError::Unknown(_) => EX_SOFTWARE,
        })
    }
}

fn status_to_exit_code(status: StatusCode) -> u8 {
    if status.is_client_error() {
        // 4xx
        match status.as_u16() {
            400 => EX_USAGE,    // Bad Request
            401 => EX_NOPERM,   // Unauthorized
            403 => EX_NOPERM,   // Forbidden
            404 => EX_NOUSER,   // Not Found (can mean resource or user)
            408 => EX_TEMPFAIL, // Request Timeout (client-side)
            409 => EX_DATAERR,  // Conflict (data state issue)
            422 => EX_DATAERR,  // Unprocessable Entity (validation error)
            429 => EX_TEMPFAIL, // Too Many Requests (rate limiting)
            _ => EX_DATAERR,    // Other 4xx client errors
        }
    } else if status.is_server_error() {
        // 5xx
        match status.as_u16() {
            500 => EX_SOFTWARE,    // Internal Server Error
            501 => EX_UNAVAILABLE, // Not Implemented
            502 => EX_UNAVAILABLE, // Bad Gateway
            503 => EX_UNAVAILABLE, // Service Unavailable
            504 => EX_TEMPFAIL,    // Gateway Timeout
            _ => EX_UNAVAILABLE,   // Other 5xx server errors
        }
    } else {
        // Non-error status, treat as a general software error
        EX_SOFTWARE
    }
}
