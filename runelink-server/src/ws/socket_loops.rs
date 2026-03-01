use axum::{
    extract::{
        State,
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use runelink_client::util::host_from_issuer;
use runelink_types::{
    user::UserRef,
    ws::{ClientWsEnvelope, FederationWsEnvelope},
};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::protocol::Message as WsMessage,
};
use uuid::Uuid;

use super::handlers::{handle_client_message, handle_federation_message};
use crate::{auth::Principal, state::AppState};

pub enum FederationSocket {
    Inbound(WebSocket),
    Outbound(WebSocketStream<MaybeTlsStream<TcpStream>>),
}

enum FederationIncomingEvent {
    Text(String),
    Closed,
    Ignored,
    Error(String),
}

impl FederationSocket {
    async fn send_text(&mut self, payload: String) -> Result<(), String> {
        match self {
            FederationSocket::Inbound(socket) => socket
                .send(AxumMessage::Text(payload.into()))
                .await
                .map_err(|error| error.to_string()),
            FederationSocket::Outbound(socket) => socket
                .send(WsMessage::Text(payload.into()))
                .await
                .map_err(|error| error.to_string()),
        }
    }

    async fn recv_event(&mut self) -> FederationIncomingEvent {
        match self {
            FederationSocket::Inbound(socket) => match socket.recv().await {
                Some(Ok(AxumMessage::Text(payload))) => {
                    FederationIncomingEvent::Text(payload.to_string())
                }
                Some(Ok(AxumMessage::Close(_))) | None => {
                    FederationIncomingEvent::Closed
                }
                Some(Ok(AxumMessage::Binary(_)))
                | Some(Ok(AxumMessage::Ping(_)))
                | Some(Ok(AxumMessage::Pong(_))) => {
                    FederationIncomingEvent::Ignored
                }
                Some(Err(error)) => {
                    FederationIncomingEvent::Error(error.to_string())
                }
            },
            FederationSocket::Outbound(socket) => match socket.next().await {
                Some(Ok(WsMessage::Text(payload))) => {
                    FederationIncomingEvent::Text(payload.to_string())
                }
                Some(Ok(WsMessage::Close(_))) | None => {
                    FederationIncomingEvent::Closed
                }
                Some(Ok(WsMessage::Binary(_)))
                | Some(Ok(WsMessage::Ping(_)))
                | Some(Ok(WsMessage::Pong(_)))
                | Some(Ok(WsMessage::Frame(_))) => {
                    FederationIncomingEvent::Ignored
                }
                Some(Err(error)) => {
                    FederationIncomingEvent::Error(error.to_string())
                }
            },
        }
    }
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
    ws.on_upgrade(move |socket| {
        federation_ws_upgrade_loop(state, headers, socket)
    })
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
                        if let Err(error) = socket.send(AxumMessage::Text(payload.into())).await {
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
                    Some(Ok(AxumMessage::Text(payload))) => {
                        match serde_json::from_str::<ClientWsEnvelope>(&payload) {
                            Ok(message) => handle_client_message(&state, conn_id, message).await,
                            Err(error) => {
                                log::warn!("Failed to parse client websocket message: {error}");
                            }
                        }
                    }
                    Some(Ok(AxumMessage::Close(_))) | None => break,
                    Some(Ok(AxumMessage::Binary(_))) | Some(Ok(AxumMessage::Ping(_))) | Some(Ok(AxumMessage::Pong(_))) => {}
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

async fn federation_ws_upgrade_loop(
    state: AppState,
    headers: HeaderMap,
    socket: WebSocket,
) {
    let (sender, outbound_rx) =
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

    federation_socket_loop(
        state,
        conn_id,
        FederationSocket::Inbound(socket),
        outbound_rx,
    )
    .await;
}

pub async fn federation_socket_loop(
    state: AppState,
    conn_id: Uuid,
    mut socket: FederationSocket,
    mut outbound_rx: mpsc::UnboundedReceiver<FederationWsEnvelope>,
) {
    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(envelope) = outbound else {
                    break;
                };
                match serde_json::to_string(&envelope) {
                    Ok(payload) => {
                        if let Err(error) = socket.send_text(payload).await {
                            log::warn!("Federation websocket send error: {error}");
                            break;
                        }
                    }
                    Err(error) => {
                        log::warn!("Failed to serialize federation websocket message: {error}");
                    }
                }
            }
            incoming = socket.recv_event() => {
                match incoming {
                    FederationIncomingEvent::Text(payload) => {
                        match serde_json::from_str::<FederationWsEnvelope>(&payload) {
                            Ok(message) => {
                                handle_federation_message(&state, conn_id, message).await;
                            }
                            Err(error) => {
                                log::warn!("Failed to parse federation websocket message: {error}");
                            }
                        }
                    }
                    FederationIncomingEvent::Closed => break,
                    FederationIncomingEvent::Ignored => {}
                    FederationIncomingEvent::Error(error) => {
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
