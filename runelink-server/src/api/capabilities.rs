use axum::{
    Json,
    extract::{MatchedPath, Request},
    http::{Method, StatusCode, header::HOST},
    middleware::Next,
    response::{IntoResponse, Response},
};
use runelink_types::capability::{
    Capability, CapabilityDiscovery, HTTP_CAPABILITY_HEADER,
    VersionedCapability, catalog_supports, supported_capabilities,
    versions::{
        NEGOTIATION_VERSION, auth, channels, memberships, messages, servers,
        users,
    },
};
use serde::Serialize;

use crate::capabilities::{client_websocket, federation_websocket, http};

pub async fn discovery() -> Json<CapabilityDiscovery> {
    Json(CapabilityDiscovery {
        negotiation_version: NEGOTIATION_VERSION,
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        http: supported_capabilities(http),
        client_websocket: supported_capabilities(client_websocket),
        federation_websocket: supported_capabilities(federation_websocket),
    })
}

pub async fn require_capability(request: Request, next: Next) -> Response {
    let server = request
        .headers()
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or("This server")
        .to_owned();
    let Some(path) = request.extensions().get::<MatchedPath>() else {
        return next.run(request).await;
    };
    let Some(required) = required_capability(request.method(), path.as_str())
    else {
        return next.run(request).await;
    };
    let Some(value) = request.headers().get(HTTP_CAPABILITY_HEADER) else {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "missing_capability",
            format!("Missing {HTTP_CAPABILITY_HEADER} header"),
        );
    };
    let Ok(value) = value.to_str() else {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "invalid_capability",
            format!("Invalid {HTTP_CAPABILITY_HEADER} header"),
        );
    };
    let Some((capability, version)) = value.rsplit_once('@') else {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "invalid_capability",
            format!("Invalid {HTTP_CAPABILITY_HEADER} header"),
        );
    };
    let Ok(version) = version.parse() else {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "invalid_capability",
            format!("Invalid {HTTP_CAPABILITY_HEADER} header"),
        );
    };
    let capability = Capability::from(capability);
    if capability != required.capability {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "unsupported_capability",
            format!("Expected the {} capability", required.capability),
        );
    }
    let requested = capability.with_version(version);
    if !required.supports(&requested) || !catalog_supports(http, &requested) {
        return capability_error(
            StatusCode::BAD_REQUEST,
            "unsupported_capability",
            format!(
                "{server} does not support the {} capability",
                requested.capability
            ),
        );
    }
    next.run(request).await
}

fn required_capability(
    method: &Method,
    path: &str,
) -> Option<RequiredCapability> {
    match (method, path) {
        (&Method::POST, "/auth/signup") => {
            required(Capability::AuthSignup, auth::SIGNUP_VERSIONS)
        }
        (&Method::POST, "/auth/token") => {
            required(Capability::AuthToken, auth::TOKEN_VERSIONS)
        }
        (&Method::GET, "/users")
        | (&Method::GET, "/users/{host}/{name}")
        | (&Method::GET, "/users/{host}/{name}/hosts") => {
            required(Capability::UsersRead, users::READ_VERSIONS)
        }
        (&Method::POST, "/users") => {
            required(Capability::UsersCreate, users::CREATE_VERSIONS)
        }
        (&Method::DELETE, "/users/{host}/{name}") => {
            required(Capability::UsersDelete, users::DELETE_VERSIONS)
        }
        (&Method::GET, "/users/{host}/{name}/servers")
        | (&Method::GET, "/servers/{server_id}/users")
        | (&Method::GET, "/servers/{server_id}/users/{host}/{name}") => {
            required(Capability::MembershipsRead, memberships::READ_VERSIONS)
        }
        (&Method::POST, "/servers/{server_id}/users")
        | (&Method::DELETE, "/servers/{server_id}/users/{host}/{name}") => {
            required(Capability::MembershipsWrite, memberships::WRITE_VERSIONS)
        }
        (&Method::GET, "/servers")
        | (&Method::GET, "/servers/{server_id}")
        | (&Method::GET, "/servers/{server_id}/with_channels") => {
            required(Capability::ServersRead, servers::READ_VERSIONS)
        }
        (&Method::POST, "/servers") => {
            required(Capability::ServersCreate, servers::CREATE_VERSIONS)
        }
        (&Method::DELETE, "/servers/{server_id}") => {
            required(Capability::ServersDelete, servers::DELETE_VERSIONS)
        }
        (&Method::GET, "/channels")
        | (&Method::GET, "/servers/{server_id}/channels")
        | (&Method::GET, "/servers/{server_id}/channels/{channel_id}") => {
            required(Capability::ChannelsRead, channels::READ_VERSIONS)
        }
        (&Method::POST, "/servers/{server_id}/channels") => {
            required(Capability::ChannelsCreate, channels::CREATE_VERSIONS)
        }
        (&Method::DELETE, "/servers/{server_id}/channels/{channel_id}") => {
            required(Capability::ChannelsDelete, channels::DELETE_VERSIONS)
        }
        (&Method::GET, "/messages")
        | (&Method::GET, "/servers/{server_id}/messages")
        | (
            &Method::GET,
            "/servers/{server_id}/channels/{channel_id}/messages",
        )
        | (
            &Method::GET,
            "/servers/{server_id}/channels/{channel_id}/messages/{message_id}",
        ) => required(Capability::MessagesRead, messages::READ_VERSIONS),
        (
            &Method::POST,
            "/servers/{server_id}/channels/{channel_id}/messages",
        ) => required(Capability::MessagesCreate, messages::CREATE_VERSIONS),
        (
            &Method::DELETE,
            "/servers/{server_id}/channels/{channel_id}/messages/{message_id}",
        ) => required(Capability::MessagesDelete, messages::DELETE_VERSIONS),
        _ => None,
    }
}

fn required(
    capability: Capability,
    versions: &'static [u16],
) -> Option<RequiredCapability> {
    Some(RequiredCapability {
        capability,
        versions,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct RequiredCapability {
    capability: Capability,
    versions: &'static [u16],
}

impl RequiredCapability {
    fn supports(&self, requested: &VersionedCapability) -> bool {
        self.capability == requested.capability
            && self.versions.contains(&requested.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_http_operations_to_semantic_capabilities() {
        assert_eq!(
            required_capability(&Method::POST, "/auth/token"),
            required(Capability::AuthToken, auth::TOKEN_VERSIONS)
        );
        assert_eq!(
            required_capability(
                &Method::GET,
                "/servers/{server_id}/channels/{channel_id}/messages",
            ),
            required(Capability::MessagesRead, messages::READ_VERSIONS)
        );
        assert_eq!(
            required_capability(
                &Method::POST,
                "/servers/{server_id}/channels/{channel_id}/messages",
            ),
            required(Capability::MessagesCreate, messages::CREATE_VERSIONS)
        );
        assert_eq!(
            required_capability(
                &Method::DELETE,
                "/servers/{server_id}/channels/{channel_id}/messages/{message_id}",
            ),
            required(Capability::MessagesDelete, messages::DELETE_VERSIONS)
        );
    }

    #[test]
    fn accepts_any_required_version() {
        let required = RequiredCapability {
            capability: Capability::MessagesRead,
            versions: &[1, 2],
        };

        assert!(required.supports(&Capability::MessagesRead.with_version(1)));
        assert!(required.supports(&Capability::MessagesRead.with_version(2)));
        assert!(!required.supports(&Capability::MessagesRead.with_version(3)));
    }

    #[test]
    fn leaves_bootstrap_endpoints_unversioned() {
        assert_eq!(required_capability(&Method::GET, "/ping"), None);
        assert_eq!(
            required_capability(&Method::GET, "/.well-known/runelink"),
            None
        );
    }
}

#[derive(Serialize)]
struct CapabilityError {
    code: &'static str,
    message: String,
}

fn capability_error(
    status: StatusCode,
    code: &'static str,
    message: String,
) -> Response {
    (status, Json(CapabilityError { code, message })).into_response()
}
