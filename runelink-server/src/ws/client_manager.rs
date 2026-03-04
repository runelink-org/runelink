#![allow(dead_code)]

use std::borrow::Borrow;

use runelink_types::{
    ids::{EventId, RequestId},
    user::UserRef,
    ws::{ClientWsEnvelope, ClientWsReply, ClientWsUpdate, WsError},
};
use tokio::sync::mpsc;

use super::pools::ClientWsPool;
use crate::ids::ConnId;

/// High-level manager for client websocket connections.
///
/// This wraps the low-level pool and exposes typed APIs so call sites do not
/// need to construct websocket envelopes manually.
#[derive(Clone, Debug, Default)]
pub struct ClientWsManager {
    pool: ClientWsPool,
}

impl ClientWsManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_connection(
        &self,
        sender: mpsc::UnboundedSender<ClientWsEnvelope>,
    ) -> ConnId {
        let conn_id = ConnId::new();
        self.pool.register_connection(conn_id, sender).await;
        conn_id
    }

    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        user_ref: UserRef,
    ) -> bool {
        self.pool.authenticate_connection(conn_id, user_ref).await
    }

    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        self.pool.deregister_connection(conn_id).await
    }

    pub async fn authenticated_user_ref(
        &self,
        conn_id: ConnId,
    ) -> Option<UserRef> {
        self.pool.authenticated_user_ref(conn_id).await
    }

    pub async fn send_update_to_connection(
        &self,
        conn_id: ConnId,
        update: ClientWsUpdate,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Update {
                    event_id: EventId::new(),
                    update,
                },
            )
            .await
    }

    pub async fn send_update_to_user(
        &self,
        user_ref: &UserRef,
        update: ClientWsUpdate,
    ) -> usize {
        self.pool
            .send_to_user(
                user_ref,
                ClientWsEnvelope::Update {
                    event_id: EventId::new(),
                    update,
                },
            )
            .await
    }

    pub async fn send_update_to_users<I, S>(
        &self,
        users: I,
        update: ClientWsUpdate,
    ) -> usize
    where
        I: IntoIterator<Item = S>,
        S: Borrow<UserRef>,
    {
        self.pool
            .send_to_users(
                users,
                ClientWsEnvelope::Update {
                    event_id: EventId::new(),
                    update,
                },
            )
            .await
    }

    pub async fn send_reply_to_connection(
        &self,
        conn_id: ConnId,
        request_id: RequestId,
        reply: ClientWsReply,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Reply {
                    request_id,
                    event_id: EventId::new(),
                    reply,
                },
            )
            .await
    }

    pub async fn send_error_to_connection(
        &self,
        conn_id: ConnId,
        request_id: Option<RequestId>,
        error: WsError,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Error {
                    request_id,
                    event_id: EventId::new(),
                    error,
                },
            )
            .await
    }

    pub async fn broadcast_update(&self, update: ClientWsUpdate) -> usize {
        self.pool
            .broadcast(ClientWsEnvelope::Update {
                event_id: EventId::new(),
                update,
            })
            .await
    }
}
