#![allow(dead_code)]

use runelink_types::{
    user::UserRef,
    ws::{ClientWsEnvelope, ClientWsReply, ClientWsUpdate, WsError},
};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::pools::ClientWsPool;

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
    ) -> Uuid {
        let conn_id = Uuid::new_v4();
        self.pool.register_connection(conn_id, sender).await;
        conn_id
    }

    pub async fn authenticate_connection(
        &self,
        conn_id: Uuid,
        user_ref: UserRef,
    ) -> bool {
        self.pool.authenticate_connection(conn_id, user_ref).await
    }

    pub async fn deregister_connection(&self, conn_id: Uuid) -> bool {
        self.pool.deregister_connection(conn_id).await
    }

    pub async fn send_update_to_connection(
        &self,
        conn_id: Uuid,
        update: ClientWsUpdate,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Update {
                    event_id: Uuid::new_v4(),
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
                    event_id: Uuid::new_v4(),
                    update,
                },
            )
            .await
    }

    pub async fn send_reply_to_connection(
        &self,
        conn_id: Uuid,
        request_id: Uuid,
        reply: ClientWsReply,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Reply {
                    request_id,
                    event_id: Uuid::new_v4(),
                    reply,
                },
            )
            .await
    }

    pub async fn send_error_to_connection(
        &self,
        conn_id: Uuid,
        request_id: Option<Uuid>,
        error: WsError,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                ClientWsEnvelope::Error {
                    request_id,
                    event_id: Uuid::new_v4(),
                    error,
                },
            )
            .await
    }

    pub async fn broadcast_update(&self, update: ClientWsUpdate) -> usize {
        self.pool
            .broadcast(ClientWsEnvelope::Update {
                event_id: Uuid::new_v4(),
                update,
            })
            .await
    }
}
