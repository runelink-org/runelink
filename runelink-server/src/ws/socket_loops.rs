use std::time::Duration;

use axum::{
    extract::{
        State,
        ws::{
            CloseFrame as AxumCloseFrame, Message as AxumMessage, WebSocket,
            WebSocketUpgrade, close_code,
        },
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use runelink_client::util::host_from_issuer;
use runelink_types::{
    capability::{
        CapabilityWelcome, NegotiatedCapabilities, negotiate_capabilities,
        supported_capabilities, versions::NEGOTIATION_VERSION,
    },
    user::UserRef,
    ws::{ClientWsEnvelope, FederationWsEnvelope},
};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::protocol::{
        CloseFrame as WsCloseFrame, Message as WsMessage,
        frame::coding::CloseCode,
    },
};

use super::handlers::{handle_client_message, handle_federation_message};
use crate::{
    auth::Principal,
    capabilities::{
        client_websocket, federation_websocket, log_version_mismatches,
    },
    ids::ConnId,
    state::AppState,
};

pub(super) const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
// How long to wait for the peer's close reply, so the close frame is not lost
// to a connection reset.
const CLOSE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(1);

pub enum FederationSocket {
    Inbound(WebSocket),
    Outbound(WebSocketStream<MaybeTlsStream<TcpStream>>),
}

pub(super) enum FederationIncomingEvent {
    Text(String),
    Closed,
    Ignored,
    Error(String),
}

impl FederationSocket {
    pub(super) async fn send_text(
        &mut self,
        payload: String,
    ) -> Result<(), String> {
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

    /// Closes the socket after a failed capability negotiation.
    pub(super) async fn close_protocol_error(&mut self, reason: &str) {
        match self {
            FederationSocket::Inbound(socket) => {
                close_axum_protocol_error(socket, reason).await;
            }
            FederationSocket::Outbound(socket) => {
                let _ = socket
                    .send(WsMessage::Close(Some(WsCloseFrame {
                        code: CloseCode::Protocol,
                        reason: reason.into(),
                    })))
                    .await;
                let _ = tokio::time::timeout(CLOSE_HANDSHAKE_TIMEOUT, async {
                    while matches!(
                        socket.next().await,
                        Some(Ok(message)) if !message.is_close()
                    ) {}
                })
                .await;
            }
        }
    }

    pub(super) async fn recv_event(&mut self) -> FederationIncomingEvent {
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
    let Some(capabilities) = negotiate_client_socket(&mut socket).await else {
        return;
    };
    let (sender, mut outbound_rx) =
        mpsc::unbounded_channel::<ClientWsEnvelope>();
    let conn_id = state
        .client_ws_manager
        .register_connection(sender, capabilities)
        .await;

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
    let auth = match Principal::from_federation_headers(&headers, &state).await
    {
        Ok(Principal::Federation(auth)) => auth,
        Ok(Principal::Client(_)) => {
            log::warn!("Rejected federation websocket with client credentials");
            return;
        }
        Err(error) => {
            log::warn!(
                "Rejected unauthenticated federation websocket: {error}"
            );
            return;
        }
    };
    let host = host_from_issuer(&auth.claims.iss);
    let mut socket = FederationSocket::Inbound(socket);
    let Some(capabilities) =
        negotiate_federation_socket(&host, &mut socket).await
    else {
        return;
    };
    let (sender, outbound_rx) =
        mpsc::unbounded_channel::<FederationWsEnvelope>();
    let conn_id = state
        .federation_ws_manager
        .register_connection(sender, capabilities)
        .await;

    let issuer = auth.claims.iss.clone();
    let _ = state
        .federation_ws_manager
        .authenticate_connection(conn_id, host, issuer)
        .await;

    federation_socket_loop(state, conn_id, socket, outbound_rx).await;
}

async fn negotiate_client_socket(
    socket: &mut WebSocket,
) -> Option<NegotiatedCapabilities> {
    let message = tokio::time::timeout(HELLO_TIMEOUT, socket.recv()).await;
    let payload = match message {
        Ok(Some(Ok(AxumMessage::Text(payload)))) => payload,
        // The client went away during the handshake.
        Ok(None | Some(Ok(AxumMessage::Close(_))) | Some(Err(_))) => {
            return None;
        }
        // Timeouts and non-text frames are not evidence of incompatibility, so
        // the client is not told it is incompatible.
        _ => {
            log::warn!("Client websocket did not send a capability hello");
            return None;
        }
    };
    let hello = match serde_json::from_str(&payload) {
        Ok(ClientWsEnvelope::Hello(hello))
            if hello.negotiation_version == NEGOTIATION_VERSION =>
        {
            hello
        }
        _ => {
            let reason = hello_rejection(&payload);
            log::warn!("Rejected client websocket: {reason}");
            close_axum_protocol_error(socket, &reason).await;
            return None;
        }
    };
    let capabilities = negotiate_capabilities(
        &hello.capabilities,
        &supported_capabilities(client_websocket),
    );
    log_version_mismatches(
        "Client websocket",
        &hello.capabilities,
        &capabilities,
        client_websocket,
    );
    let welcome = ClientWsEnvelope::Welcome(CapabilityWelcome {
        negotiation_version: NEGOTIATION_VERSION,
        capabilities: capabilities.clone(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
    });
    let payload = serde_json::to_string(&welcome).ok()?;
    socket.send(AxumMessage::Text(payload.into())).await.ok()?;
    Some(capabilities)
}

/// Explains why a first message is not an acceptable hello. The negotiation
/// version is read on its own, so hellos from newer negotiation versions get an
/// accurate reason even if the rest of their shape changed.
fn hello_rejection(payload: &str) -> String {
    let version = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| value.pointer("/data/negotiation_version")?.as_u64());
    match version {
        Some(version) if version != u64::from(NEGOTIATION_VERSION) => {
            format!("Unsupported negotiation version {version}")
        }
        _ => "Invalid capability hello".into(),
    }
}

/// Closes a socket whose capability negotiation failed, so the peer can tell an
/// incompatible host apart from a dropped connection.
async fn close_axum_protocol_error(socket: &mut WebSocket, reason: &str) {
    let _ = socket
        .send(AxumMessage::Close(Some(AxumCloseFrame {
            code: close_code::PROTOCOL,
            reason: reason.into(),
        })))
        .await;
    let _ = tokio::time::timeout(CLOSE_HANDSHAKE_TIMEOUT, async {
        while matches!(
            socket.recv().await,
            Some(Ok(message)) if !matches!(message, AxumMessage::Close(_))
        ) {}
    })
    .await;
}

async fn negotiate_federation_socket(
    host: &str,
    socket: &mut FederationSocket,
) -> Option<NegotiatedCapabilities> {
    let event = tokio::time::timeout(HELLO_TIMEOUT, socket.recv_event()).await;
    let payload = match event {
        Ok(FederationIncomingEvent::Text(payload)) => payload,
        // The peer went away during the handshake.
        Ok(
            FederationIncomingEvent::Closed | FederationIncomingEvent::Error(_),
        ) => {
            return None;
        }
        // Timeouts and non-text frames are not evidence of incompatibility, so
        // the peer is not told it is incompatible.
        _ => {
            log::warn!(
                "Federation websocket from {host} did not send a capability hello"
            );
            return None;
        }
    };
    let hello = match serde_json::from_str(&payload) {
        Ok(FederationWsEnvelope::Hello(hello))
            if hello.negotiation_version == NEGOTIATION_VERSION =>
        {
            hello
        }
        _ => {
            let reason = hello_rejection(&payload);
            log::warn!("Rejected federation websocket from {host}: {reason}");
            socket.close_protocol_error(&reason).await;
            return None;
        }
    };
    let capabilities = negotiate_capabilities(
        &hello.capabilities,
        &supported_capabilities(federation_websocket),
    );
    log_version_mismatches(
        &format!("Federation peer {host}"),
        &hello.capabilities,
        &capabilities,
        federation_websocket,
    );
    let welcome = FederationWsEnvelope::Welcome(CapabilityWelcome {
        negotiation_version: NEGOTIATION_VERSION,
        capabilities: capabilities.clone(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
    });
    socket
        .send_text(serde_json::to_string(&welcome).ok()?)
        .await
        .ok()?;
    Some(capabilities)
}

pub async fn federation_socket_loop(
    state: AppState,
    conn_id: ConnId,
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

#[cfg(test)]
mod tests {
    use super::hello_rejection;

    #[test]
    fn hello_rejection_reports_newer_negotiation_versions() {
        assert_eq!(
            hello_rejection(
                r#"{"type":"hello","data":{"negotiation_version":2,"offers":[]}}"#
            ),
            "Unsupported negotiation version 2"
        );
        assert_eq!(
            hello_rejection(
                r#"{"type":"hello","data":{"negotiation_version":1}}"#
            ),
            "Invalid capability hello"
        );
        assert_eq!(hello_rejection("not json"), "Invalid capability hello");
    }
}
