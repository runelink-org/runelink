use runelink_types::ws::{ClientWsEnvelope, FederationWsEnvelope};

use crate::state::AppState;

pub async fn handle_client_message(
    _state: &AppState,
    message: ClientWsEnvelope,
) {
    match message {
        ClientWsEnvelope::Request { .. } => {
            todo!("handle client websocket request message")
        }
        ClientWsEnvelope::Reply { .. } => {
            todo!("handle client websocket reply message")
        }
        ClientWsEnvelope::Error { .. } => {
            todo!("handle client websocket error message")
        }
        ClientWsEnvelope::Update { .. } => {
            todo!("handle client websocket update message")
        }
    }
}

pub async fn handle_federation_message(
    _state: &AppState,
    message: FederationWsEnvelope,
) {
    match message {
        FederationWsEnvelope::Request { .. } => {
            todo!("handle federation websocket request message")
        }
        FederationWsEnvelope::Reply { .. } => {
            todo!("handle federation websocket reply message")
        }
        FederationWsEnvelope::Error { .. } => {
            todo!("handle federation websocket error message")
        }
        FederationWsEnvelope::Update { .. } => {
            todo!("handle federation websocket update message")
        }
    }
}
