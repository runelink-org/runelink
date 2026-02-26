use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use runelink_types::ws::{ClientWsEnvelope, FederationWsEnvelope};

use super::handlers::{handle_client_message, handle_federation_message};
use crate::state::AppState;

pub async fn client_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_ws_loop(state, socket))
}

pub async fn federation_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| federation_ws_loop(state, socket))
}

async fn client_ws_loop(state: AppState, mut socket: WebSocket) {
    while let Some(result) = socket.recv().await {
        match result {
            Ok(Message::Text(payload)) => {
                match serde_json::from_str::<ClientWsEnvelope>(&payload) {
                    Ok(message) => handle_client_message(&state, message).await,
                    Err(error) => {
                        log::warn!(
                            "Failed to parse client websocket message: {error}"
                        );
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Binary(_))
            | Ok(Message::Ping(_))
            | Ok(Message::Pong(_)) => {}
            Err(error) => {
                log::warn!("Client websocket receive error: {error}");
                break;
            }
        }
    }
}

async fn federation_ws_loop(state: AppState, mut socket: WebSocket) {
    while let Some(result) = socket.recv().await {
        match result {
            Ok(Message::Text(payload)) => {
                match serde_json::from_str::<FederationWsEnvelope>(&payload) {
                    Ok(message) => {
                        handle_federation_message(&state, message).await;
                    }
                    Err(error) => {
                        log::warn!(
                            "Failed to parse federation websocket message: {error}"
                        );
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Binary(_))
            | Ok(Message::Ping(_))
            | Ok(Message::Pong(_)) => {}
            Err(error) => {
                log::warn!("Federation websocket receive error: {error}");
                break;
            }
        }
    }
}
