mod client;
mod federation;
mod shared;

use std::sync::Arc;

use runelink_types::{
    capability::{Capability, CapabilityVersion, preferred_version},
    ids::RequestId,
    user::UserRef,
    ws::{
        ClientWsEnvelope, ClientWsRequest, FederationWsEnvelope,
        FederationWsRequest, WsError,
    },
};
use tokio::sync::Semaphore;

use crate::{
    admission,
    capabilities::{client_websocket, federation_websocket},
    ids::ConnId,
    state::AppState,
};

/// Requests beyond this many in flight on one connection wait for room.
pub const MAX_IN_FLIGHT_CLIENT_REQUESTS: usize = 32;

/// Dispatch a client websocket message, waiting only until it is admitted.
/// Requests that change connection state are never admitted early, so they
/// finish before later requests are read.
pub async fn handle_client_message(
    state: &AppState,
    conn_id: ConnId,
    message: ClientWsEnvelope,
    in_flight: &Arc<Semaphore>,
) {
    match message {
        ClientWsEnvelope::Request {
            request_id,
            request,
        } => {
            let permit = in_flight
                .clone()
                .acquire_owned()
                .await
                .expect("the in-flight semaphore is never closed");
            let state = state.clone();
            let (request, admitted) = admission::track(async move {
                handle_client_request(&state, conn_id, request_id, request)
                    .await;
                drop(permit);
            });
            tokio::spawn(request);
            admitted.await;
        }
        ClientWsEnvelope::Reply { .. } => {
            log::warn!("Ignoring client websocket reply envelope");
        }
        ClientWsEnvelope::Error { .. } => {
            log::warn!("Ignoring client websocket error envelope");
        }
        ClientWsEnvelope::Update { .. } => {
            log::warn!("Ignoring client websocket update envelope");
        }
        ClientWsEnvelope::Hello(_) | ClientWsEnvelope::Welcome(_) => {
            log::warn!("Ignoring repeated client capability negotiation");
        }
    }
}

async fn handle_client_request(
    state: &AppState,
    conn_id: ConnId,
    request_id: RequestId,
    request: ClientWsRequest,
) {
    let capability = request.capability();
    let version = preferred_version(client_websocket, &capability);
    let Some(required) =
        version.map(|version| capability.with_version(version))
    else {
        let _ = state
            .client_ws_manager
            .send_error_to_connection(
                conn_id,
                Some(request_id),
                unsupported_capability(
                    &capability,
                    None,
                    &state.config.public_host_with_explicit_port(),
                ),
            )
            .await;
        return;
    };
    if !state
        .client_ws_manager
        .supports_capability(conn_id, &required)
        .await
    {
        let _ = state
            .client_ws_manager
            .send_error_to_connection(
                conn_id,
                Some(request_id),
                unsupported_capability(
                    &capability,
                    version,
                    &state.config.public_host_with_explicit_port(),
                ),
            )
            .await;
        return;
    }
    let result = client::handle_client_request(state, conn_id, request).await;
    match result {
        Ok(reply) => {
            let sent = state
                .client_ws_manager
                .send_reply_to_connection(conn_id, request_id, reply)
                .await;
            if !sent {
                log::warn!(
                    "Failed to send client websocket reply for request {request_id}"
                );
            }
        }
        Err(error) => {
            let sent = state
                .client_ws_manager
                .send_error_to_connection(
                    conn_id,
                    Some(request_id),
                    error.into(),
                )
                .await;
            if !sent {
                log::warn!(
                    "Failed to send client websocket error for request {request_id}"
                );
            }
        }
    }
}

/// Dispatch a federation websocket message, waiting only until a request is
/// admitted, like client requests.
///
/// Updates and responses are handled before the next message is read, which
/// never waits on the network, so requests waiting on a peer always make
/// progress. Requests are not limited, since waiting for room here could also
/// hold up the replies that the running requests depend on.
pub async fn handle_federation_message(
    state: &AppState,
    conn_id: ConnId,
    message: FederationWsEnvelope,
) {
    match message {
        FederationWsEnvelope::Request {
            request_id,
            delegated_user_ref,
            request,
            ..
        } => {
            let state = state.clone();
            let (request, admitted) = admission::track(async move {
                handle_federation_request(
                    &state,
                    conn_id,
                    request_id,
                    delegated_user_ref,
                    request,
                )
                .await;
            });
            tokio::spawn(request);
            admitted.await;
        }
        // Handled before the next message from this connection is read. A
        // host sends its updates and replies on one connection, so a reply
        // resolves only after the updates the peer sent before it, which are
        // all that write what is cached about its servers.
        response_envelope @ (FederationWsEnvelope::Reply { .. }
        | FederationWsEnvelope::Error { .. }) => {
            let resolved = state
                .federation_ws_manager
                .resolve_response_envelope(response_envelope)
                .await;
            if !resolved {
                log::warn!("Unmatched federation websocket response envelope");
            }
        }
        FederationWsEnvelope::Update { update, .. } => {
            let capability = update.capability();
            let Some(required) =
                preferred_version(federation_websocket, &capability)
                    .map(|version| capability.with_version(version))
            else {
                log::warn!(
                    "Ignoring update for unsupported capability {capability}"
                );
                return;
            };
            if !state
                .federation_ws_manager
                .supports_capability(conn_id, &required)
                .await
            {
                log::warn!(
                    "Ignoring update for unnegotiated capability {}",
                    update.capability()
                );
                return;
            }
            let Some(sender) = state
                .federation_ws_manager
                .authenticated_host(conn_id)
                .await
            else {
                log::warn!("Ignoring update from unauthenticated connection");
                return;
            };
            if let Err(error) =
                federation::handle_federation_update(state, &sender, update)
                    .await
            {
                log::warn!(
                    "Failed handling federation websocket update: {error}"
                );
            }
        }
        FederationWsEnvelope::Hello(_) | FederationWsEnvelope::Welcome(_) => {
            log::warn!("Ignoring repeated federation capability negotiation");
        }
    }
}

async fn handle_federation_request(
    state: &AppState,
    conn_id: ConnId,
    request_id: RequestId,
    delegated_user_ref: Option<UserRef>,
    request: FederationWsRequest,
) {
    // Captured now, so that the response still reaches the host if this
    // connection closes meanwhile.
    let host = state
        .federation_ws_manager
        .authenticated_host(conn_id)
        .await;
    let host = host.as_deref();
    let capability = request.capability();
    let version = preferred_version(federation_websocket, &capability);
    let Some(required) =
        version.map(|version| capability.with_version(version))
    else {
        let _ = state
            .federation_ws_manager
            .send_error(
                conn_id,
                host,
                Some(request_id),
                unsupported_capability(
                    &capability,
                    None,
                    &state.config.public_host_with_explicit_port(),
                ),
            )
            .await;
        return;
    };
    if !state
        .federation_ws_manager
        .supports_capability(conn_id, &required)
        .await
    {
        let _ = state
            .federation_ws_manager
            .send_error(
                conn_id,
                host,
                Some(request_id),
                unsupported_capability(
                    &capability,
                    version,
                    &state.config.public_host_with_explicit_port(),
                ),
            )
            .await;
        return;
    }
    let result = federation::handle_federation_request(
        state,
        conn_id,
        delegated_user_ref,
        request,
    )
    .await;
    match result {
        Ok(reply) => {
            let sent = state
                .federation_ws_manager
                .send_reply(conn_id, host, request_id, reply)
                .await;
            if !sent {
                log::warn!(
                    "Failed to send federation websocket reply for request {request_id}"
                );
            }
        }
        Err(error) => {
            let sent = state
                .federation_ws_manager
                .send_error(conn_id, host, Some(request_id), error.into())
                .await;
            if !sent {
                log::warn!(
                    "Failed to send federation websocket error for request {request_id}"
                );
            }
        }
    }
}

fn unsupported_capability(
    capability: &Capability,
    version: Option<CapabilityVersion>,
    server: &str,
) -> WsError {
    let message = match version {
        Some(version) => format!(
            "Capability {} was not negotiated",
            capability.with_version(version)
        ),
        None => {
            format!("{server} does not support the {capability} capability")
        }
    };
    WsError {
        code: "unsupported_capability".into(),
        message,
        details: None,
    }
}
