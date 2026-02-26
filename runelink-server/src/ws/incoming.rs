use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use runelink_types::{
    user::UserRef,
    ws::{ClientWsEnvelope, FederationWsEnvelope},
};
use tokio::sync::mpsc;

use super::handlers::{handle_client_message, handle_federation_message};
use crate::{auth::Principal, state::AppState};

fn host_from_issuer(issuer: &str) -> String {
    issuer
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string()
}

pub async fn client_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_ws_loop(state, headers, socket))
}

pub async fn federation_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| federation_ws_loop(state, headers, socket))
}

async fn client_ws_loop(
    state: AppState,
    headers: HeaderMap,
    mut socket: WebSocket,
) {
    let (sender, mut outbound_rx) =
        mpsc::unbounded_channel::<ClientWsEnvelope>();
    let conn_id = state.client_ws_manager.register_connection(sender).await;

    if let Ok(Principal::Client(auth)) =
        Principal::from_client_headers(&headers, &state)
    {
        if let Some(user_ref) = UserRef::parse_subject(&auth.claims.sub) {
            let _ = state
                .client_ws_manager
                .authenticate_connection(conn_id, user_ref)
                .await;
        }
    }

    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(envelope) = outbound else {
                    break;
                };
                match serde_json::to_string(&envelope) {
                    Ok(payload) => {
                        if let Err(error) = socket.send(Message::Text(payload.into())).await {
                            log::warn!("Client websocket send error: {error}");
                            break;
                        }
                    }
                    Err(error) => {
                        log::warn!("Failed to serialize client websocket message: {error}");
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(payload))) => {
                        match serde_json::from_str::<ClientWsEnvelope>(&payload) {
                            Ok(message) => handle_client_message(&state, conn_id, message).await,
                            Err(error) => {
                                log::warn!("Failed to parse client websocket message: {error}");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Err(error)) => {
                        log::warn!("Client websocket receive error: {error}");
                        break;
                    }
                }
            }
        }
    }

    let _ = state.client_ws_manager.deregister_connection(conn_id).await;
}

async fn federation_ws_loop(
    state: AppState,
    headers: HeaderMap,
    mut socket: WebSocket,
) {
    let (sender, mut outbound_rx) =
        mpsc::unbounded_channel::<FederationWsEnvelope>();
    let conn_id = state
        .federation_ws_manager
        .register_connection(sender)
        .await;

    if let Ok(Principal::Federation(auth)) =
        Principal::from_federation_headers(&headers, &state).await
    {
        let host = host_from_issuer(&auth.claims.iss);
        let _ = state
            .federation_ws_manager
            .authenticate_connection(conn_id, host)
            .await;
    }

    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(envelope) = outbound else {
                    break;
                };
                match serde_json::to_string(&envelope) {
                    Ok(payload) => {
                        if let Err(error) = socket.send(Message::Text(payload.into())).await {
                            log::warn!("Federation websocket send error: {error}");
                            break;
                        }
                    }
                    Err(error) => {
                        log::warn!("Failed to serialize federation websocket message: {error}");
                    }
                }
            }
            incoming = socket.recv() => {
                let Some(envelope) = incoming else {
                    break;
                };
                match envelope {
                    Ok(Message::Text(payload)) => {
                        match serde_json::from_str::<FederationWsEnvelope>(&payload) {
                            Ok(message) => {
                                handle_federation_message(&state, conn_id, message).await;
                            }
                            Err(error) => {
                                log::warn!("Failed to parse federation websocket message: {error}");
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Binary(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                    Err(error) => {
                        log::warn!("Federation websocket receive error: {error}");
                        break;
                    }
                }
            }
        }
    }

    let _ = state
        .federation_ws_manager
        .deregister_connection(conn_id)
        .await;
}
