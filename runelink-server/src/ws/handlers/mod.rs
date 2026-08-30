mod client;
mod federation;
mod shared;

use runelink_types::{
    capability::{Capability, preferred_version},
    ws::{ClientWsEnvelope, FederationWsEnvelope, WsError},
};

use crate::{
    capabilities::{client_websocket, federation_websocket},
    ids::ConnId,
    state::AppState,
};

pub async fn handle_client_message(
    state: &AppState,
    conn_id: ConnId,
    message: ClientWsEnvelope,
) {
    match message {
        ClientWsEnvelope::Request {
            request_id,
            request,
        } => {
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
            let result =
                client::handle_client_request(state, conn_id, request).await;
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
            if let Err(error) =
                federation::handle_federation_update(state, update).await
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

fn unsupported_capability(
    capability: &Capability,
    version: Option<u16>,
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
