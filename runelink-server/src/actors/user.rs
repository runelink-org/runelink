//! The user actor: creating and deleting one user's record. It stops once it
//! has had nothing to do for a while.
//!
//! A user actor may wait on server actors, to remove the user's memberships
//! before the user, but server actors never wait on user actors.

use std::time::Duration;

use runelink_types::{
    server::{ServerRef, ServerRole},
    user::{NewUser, User, UserRef},
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use tokio::sync::mpsc;

use super::{Actors, Envelope, MAILBOX_CAPACITY, UserMailbox};
use crate::{
    auth::{Authorization, Facts},
    auth_service::NAME_REUSE_DELAY,
    db::DbPool,
    error::{ApiError, ApiResult},
    queries,
};

/// How long a user actor waits for another operation before stopping.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Attempts at removing a home user's memberships before giving up, in case
/// new ones keep appearing meanwhile.
const DELETE_ATTEMPTS: usize = 3;

#[derive(Debug)]
pub(super) enum UserOp {
    /// Create a home user, with a local account if a password hash is given.
    Create {
        new_user: NewUser,
        password_hash: Option<String>,
    },
    /// Delete a home user, telling every host that knows them.
    DeleteHome,
    /// Delete the record of a remote user whose home host deleted them.
    DeleteRemote,
}

#[derive(Debug)]
pub(super) enum UserOutput {
    User(User),
    Done,
}

pub(super) fn spawn(actors: Actors, user_ref: UserRef) -> UserMailbox {
    let (mailbox, mut inbox) =
        mpsc::channel::<Envelope<UserOp, UserOutput>>(MAILBOX_CAPACITY);
    tokio::spawn(async move {
        loop {
            match tokio::time::timeout(IDLE_TIMEOUT, inbox.recv()).await {
                Ok(Some(Envelope { op, auth, reply })) => {
                    let mut facts = UserFacts {
                        db_pool: &actors.ctx.db_pool,
                    };
                    let result = match auth
                        .check(&actors.ctx.config, &mut facts)
                        .await
                    {
                        Ok(()) => execute(&actors, &user_ref, op).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                Ok(None) => return,
                // Callers take a mailbox only while holding the registry and
                // drop it once their operation is queued, so while the
                // registry is held, the registry's own copy being the only one
                // means nobody is about to queue anything.
                Err(_) => {
                    let mut registry = actors.registry.lock().await;
                    if inbox.is_empty()
                        && registry
                            .users
                            .get(&user_ref)
                            .is_some_and(|mailbox| mailbox.strong_count() == 1)
                    {
                        registry.users.remove(&user_ref);
                        return;
                    }
                }
            }
        }
    });
    mailbox
}

/// User actors know no server's members, so requirements on them stay at the
/// edge and fail here.
struct UserFacts<'a> {
    db_pool: &'a DbPool,
}

impl Facts for UserFacts<'_> {
    async fn user(&mut self, user_ref: &UserRef) -> ApiResult<Option<User>> {
        queries::users::find_by_ref(self.db_pool, user_ref.clone()).await
    }

    async fn server_role(
        &mut self,
        _server_ref: &ServerRef,
        _user_ref: &UserRef,
    ) -> ApiResult<Option<ServerRole>> {
        Ok(None)
    }
}

async fn execute(
    actors: &Actors,
    user_ref: &UserRef,
    op: UserOp,
) -> ApiResult<UserOutput> {
    let ctx = &actors.ctx;
    match op {
        UserOp::Create {
            new_user,
            password_hash,
        } => {
            let mut tx = ctx.db_pool.begin().await?;
            let user =
                queries::users::insert(&mut tx, &new_user, NAME_REUSE_DELAY)
                    .await?;
            if let Some(password_hash) = password_hash {
                queries::accounts::insert(
                    &mut *tx,
                    user.as_ref(),
                    &password_hash,
                )
                .await?;
            }
            tx.commit().await?;
            let _ = ctx
                .client_ws_manager
                .broadcast_update(ClientWsUpdate::UserUpserted(user.clone()))
                .await;
            Ok(UserOutput::User(user))
        }

        UserOp::DeleteHome => {
            delete(actors, user_ref).await?;
            ctx.client_ws_manager.deauthenticate_user(user_ref).await;
            let _ = ctx
                .client_ws_manager
                .broadcast_update(ClientWsUpdate::UserDeleted {
                    user_ref: user_ref.clone(),
                })
                .await;
            // Every connected peer, not only the hosts of the servers they are
            // known to belong to, which misses a join still in flight.
            let _ = ctx
                .federation_ws_manager
                .broadcast_update(FederationWsUpdate::RemoteUserDeleted {
                    user_ref: user_ref.clone(),
                })
                .await;
            Ok(UserOutput::Done)
        }

        UserOp::DeleteRemote => {
            // Recorded first, even for a user unknown here, so that joins in
            // flight are refused rather than keep the deletion retrying.
            queries::users::mark_deleted(&ctx.db_pool, user_ref).await?;
            match delete(actors, user_ref).await {
                Ok(()) | Err(ApiError::NotFound) => {}
                Err(error) => return Err(error),
            }
            let _ = ctx
                .client_ws_manager
                .broadcast_update(ClientWsUpdate::UserDeleted {
                    user_ref: user_ref.clone(),
                })
                .await;
            Ok(UserOutput::Done)
        }
    }
}

/// Remove the user's memberships through their servers' actors, then the user.
/// The database refuses to delete a user who still has memberships, so one
/// added meanwhile is removed on the next attempt rather than bypassing its
/// server actor.
async fn delete(actors: &Actors, user_ref: &UserRef) -> ApiResult<()> {
    let ctx = &actors.ctx;
    for _ in 0..DELETE_ATTEMPTS {
        let server_ids = queries::memberships::get_local_server_ids_for_user(
            &*ctx.db_pool,
            user_ref,
        )
        .await?;
        for server_id in server_ids {
            match actors
                .delete_membership(
                    server_id,
                    user_ref.clone(),
                    Authorization::host(),
                )
                .await
            {
                // Left or deleted meanwhile
                Ok(()) | Err(ApiError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        match queries::users::delete(
            &ctx.db_pool,
            user_ref.clone(),
            NAME_REUSE_DELAY,
        )
        .await
        {
            Err(ApiError::ForeignKeyViolation) => continue,
            result => return result,
        }
    }
    Err(ApiError::Conflict(
        "The user keeps joining servers; try again later".into(),
    ))
}
