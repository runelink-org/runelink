use std::{error::Error as StdError, sync::Arc};

use runelink_types::{capability::Capability, ws::WsError};
use serde_json::Value;
use thiserror::Error;

/// A source error, shared so that one failure can be reported to several
/// callers.
pub type Cause = Arc<dyn StdError + Send + Sync>;

/// Why a connection or request failed, or a problem outside of any one
/// request reported by [`super::Connection::errors`].
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The host replied to a request with an error. An
    /// `unsupported_capability` reply is [`Error::UnsupportedCapability`]
    /// instead.
    #[error("{0}")]
    Host(HostError),

    /// The host rejected capability negotiation, or this client rejected the
    /// host's reply. Reconnecting will not help until one side is updated, so
    /// automatic reconnection stops.
    #[error("{host} is incompatible with this client{}", suffix(reason))]
    IncompatibleHost {
        /// The host, or the websocket URL without its credentials, query, or
        /// fragment for URL connections
        host: String,
        reason: String,
    },

    /// A request needs a capability that this client does not offer, or that
    /// the host did not agree to. It fails without closing the connection.
    #[error("{}", unsupported_message(host, capability, issue.as_deref()))]
    UnsupportedCapability {
        host: String,
        capability: Capability,
        /// Why the host did not agree to it, or `None` when this client lacks
        /// it
        issue: Option<String>,
    },

    /// The websocket closed or could not open.
    #[error("WebSocket closed ({code}){}", suffix(reason))]
    Connection {
        code: u16,
        reason: String,
        #[source]
        cause: Option<Cause>,
    },

    /// [`super::Connection::disconnect`] cancelled the call, or there was no
    /// connection to send the request on.
    #[error("WebSocket disconnected")]
    Disconnected,

    /// A request could not be encoded, so it was not sent.
    #[error("Invalid request: {cause}")]
    InvalidRequest { cause: Cause },

    /// The host sent a message this client cannot use.
    #[error("{reason}")]
    Protocol {
        reason: String,
        #[source]
        cause: Option<Cause>,
    },
}

/// An error reply from the host.
#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct HostError {
    pub kind: HostErrorKind,
    /// Written by the host for people to read
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostErrorKind {
    /// The host did not authenticate the connection, or did not allow the
    /// request, such as an action that needs a server admin (`auth_error`).
    Auth,
    /// The host rejected the request as invalid (`bad_request`).
    BadRequest,
    /// A resource the request names does not exist (`not_found`).
    NotFound,
    /// The request conflicts with the resource's current state, such as an
    /// edit of an outdated version or a duplicate (`conflict`).
    Conflict,
    /// The host could not complete the request, such as when a peer host is
    /// unavailable (`internal_error`), or replied with an error code this
    /// client does not know.
    Internal { code: String },
}

impl Error {
    pub(super) fn connection(code: u16, reason: impl Into<String>) -> Self {
        Self::Connection {
            code,
            reason: reason.into(),
            cause: None,
        }
    }

    pub(super) fn protocol(reason: impl Into<String>) -> Self {
        Self::Protocol {
            reason: reason.into(),
            cause: None,
        }
    }

    /// The error for a host's error reply, by its code.
    pub(super) fn from_reply(
        error: WsError,
        host: &str,
        capability: Capability,
    ) -> Self {
        let kind = match error.code.as_str() {
            "auth_error" => HostErrorKind::Auth,
            "bad_request" => HostErrorKind::BadRequest,
            "not_found" => HostErrorKind::NotFound,
            "conflict" => HostErrorKind::Conflict,
            "unsupported_capability" => {
                return Self::UnsupportedCapability {
                    host: host.into(),
                    capability,
                    issue: Some(error.message),
                };
            }
            _ => HostErrorKind::Internal { code: error.code },
        };
        Self::Host(HostError {
            kind,
            message: error.message,
            details: error.details,
        })
    }
}

fn suffix(reason: &str) -> String {
    if reason.is_empty() {
        String::new()
    } else {
        format!(": {reason}")
    }
}

fn unsupported_message(
    host: &str,
    capability: &Capability,
    issue: Option<&str>,
) -> String {
    match issue {
        Some(issue) => format!("{host}: {issue}"),
        None => {
            format!("This client does not support the {capability} capability")
        }
    }
}

/// The connection options were invalid.
#[derive(Clone, Debug, Error)]
#[error("{0}")]
pub struct OptionsError(pub(super) String);
