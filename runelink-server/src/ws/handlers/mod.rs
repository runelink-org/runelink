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
        FederationWsRequest, FederationWsUpdate, WsError,
    },
};
use tokio::sync::{Semaphore, mpsc};

use crate::{
    capabilities::{client_websocket, federation_websocket},
    ids::ConnId,
    state::AppState,
};

/// Requests beyond this many in flight on one connection wait for room.
pub const MAX_IN_FLIGHT_CLIENT_REQUESTS: usize = 32;

/// A federation update and the host that sent it.
pub type SenderUpdate = (String, FederationWsUpdate);

/// Dispatch a client websocket message without waiting for its handling, so
/// that a slow request does not hold up the connection's other requests.
/// Connection state changes run first, for requests that follow them.
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
            if matches!(
                request,
                ClientWsRequest::AuthTokenAccess(_)
                    | ClientWsRequest::AuthTokenPassword(_)
                    | ClientWsRequest::AuthTokenRefresh(_)
            ) {
                handle_client_request(state, conn_id, request_id, request)
                    .await;
                return;
            }
            let Ok(permit) = in_flight.clone().acquire_owned().await else {
                return;
            };
            let state = state.clone();
            tokio::spawn(async move {
                handle_client_request(&state, conn_id, request_id, request)
                    .await;
                drop(permit);
            });
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

/// Dispatch a federation websocket message without waiting on its handling.
///
/// Replies resolve here, so that requests waiting on this connection always
/// make progress. Updates queue on `updates` to be applied in order. Requests
/// are not limited, since waiting for room here could also hold up the replies
/// that the running requests depend on.
pub async fn handle_federation_message(
    state: &AppState,
    conn_id: ConnId,
    message: FederationWsEnvelope,
    updates: &mpsc::UnboundedSender<SenderUpdate>,
) {
    match message {
        FederationWsEnvelope::Request {
            request_id,
            delegated_user_ref,
            request,
            ..
        } => {
            let state = state.clone();
            tokio::spawn(async move {
                handle_federation_request(
                    &state,
                    conn_id,
                    request_id,
                    delegated_user_ref,
                    request,
                )
                .await;
            });
        }
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
            let _ = updates.send((sender, update));
        }
        FederationWsEnvelope::Hello(_) | FederationWsEnvelope::Welcome(_) => {
            log::warn!("Ignoring repeated federation capability negotiation");
        }
    }
}

/// Apply a connection's federation updates one at a time, in the order the peer
/// sent them.
pub async fn apply_federation_updates(
    state: AppState,
    mut updates: mpsc::UnboundedReceiver<SenderUpdate>,
) {
    while let Some((sender, update)) = updates.recv().await {
        if let Err(error) =
            federation::handle_federation_update(&state, &sender, update).await
        {
            log::warn!("Failed handling federation websocket update: {error}");
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
    let capability = request.capability();
    let version = preferred_version(federation_websocket, &capability);
    let Some(required) =
        version.map(|version| capability.with_version(version))
    else {
        let _ = state
            .federation_ws_manager
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
        .federation_ws_manager
        .supports_capability(conn_id, &required)
        .await
    {
        let _ = state
            .federation_ws_manager
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
                .send_reply_to_connection(conn_id, request_id, reply)
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
                .send_error_to_connection(
                    conn_id,
                    Some(request_id),
                    error.into(),
                )
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
