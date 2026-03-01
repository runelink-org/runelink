mod client;
mod federation;
mod shared;

use runelink_types::ws::{ClientWsEnvelope, FederationWsEnvelope};
use uuid::Uuid;

use crate::state::AppState;

pub async fn handle_client_message(
    state: &AppState,
    conn_id: Uuid,
    message: ClientWsEnvelope,
) {
    match message {
        ClientWsEnvelope::Request {
            request_id,
            request,
        } => {
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
    }
}

pub async fn handle_federation_message(
    state: &AppState,
    conn_id: Uuid,
    message: FederationWsEnvelope,
) {
    match message {
        FederationWsEnvelope::Request {
            request_id,
            delegated_user_ref,
            request,
            ..
        } => {
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
            if let Err(error) =
                federation::handle_federation_update(state, update).await
            {
                log::warn!(
                    "Failed handling federation websocket update: {error}"
                );
            }
        }
    }
}
