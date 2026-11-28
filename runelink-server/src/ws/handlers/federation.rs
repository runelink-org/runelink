use log::debug;
use runelink_types::{
    host::Host,
    server::ServerRef,
    user::UserRef,
    ws::{
        ClientWsUpdate, FederationWsConnectionState, FederationWsReply,
        FederationWsRequest, FederationWsUpdate,
    },
};

use super::shared::authorize_federation;
use crate::{
    auth::{Authorization, Requirement},
    error::{ApiError, ApiResult},
    ids::ConnId,
    ops, queries,
    state::AppState,
};

/// Fanout a remote server update to the server's cached local members (best
/// effort). Updates for servers that are not cached are dropped.
async fn fanout_remote_server_update(
    state: &AppState,
    server_ref: &ServerRef,
    client_update: ClientWsUpdate,
) -> ApiResult<()> {
    let Some(local_users) = queries::memberships::get_remote_server_members(
        &state.db_pool,
        server_ref,
    )
    .await?
    else {
        return Ok(());
    };
    let _ = state
        .client_ws_manager
        .send_update_to_users(local_users, client_update)
        .await;
    Ok(())
}

/// Handle a federation websocket update from `sender`, which must own what
/// the update is about.
pub(super) async fn handle_federation_update(
    state: &AppState,
    sender: &Host,
    update: FederationWsUpdate,
) -> ApiResult<()> {
    debug!("WS federation: update from {sender}: {update:#?}");
    if state.config.locality(sender).is_local() {
        return Err(ApiError::AuthError(
            "Federation updates cannot come from the local host".into(),
        ));
    }
    if let Some(owner) = update.owner()
        && owner != sender
    {
        return Err(ApiError::AuthError(format!(
            "{sender} cannot send updates about resources on {owner}"
        )));
    }
    match update {
        FederationWsUpdate::MembershipUpserted(membership) => {
            // Only local users' memberships, and the servers they are in, are
            // cached. Other users may not even have a user record here, but
            // members still hear about them.
            if state.config.locality(&membership.user.host).is_local() {
                // Together, so a membership that fails leaves no server
                let mut tx = state.db_pool.begin().await?;
                queries::servers::upsert_remote(&mut *tx, &membership.server)
                    .await?;
                let inserted = queries::memberships::insert_remote(
                    &mut *tx,
                    &membership.clone().into(),
                )
                .await?;
                tx.commit().await?;
                // The server's own updates reach only members cached here, so
                // a new member, such as its creator, may not have heard of it.
                if inserted {
                    let _ = state
                        .client_ws_manager
                        .send_update_to_users(
                            [&membership.user.as_ref()],
                            ClientWsUpdate::ServerUpserted(
                                membership.server.clone(),
                            ),
                        )
                        .await;
                    state.presence.joined(
                        membership.user.as_ref(),
                        membership.server.to_ref(),
                    );
                }
            } else {
                queries::servers::update_remote(
                    &state.db_pool,
                    &membership.server,
                )
                .await?;
            }
            fanout_remote_server_update(
                state,
                &membership.server.to_ref(),
                ClientWsUpdate::MembershipUpserted(membership),
            )
            .await?;
        }

        FederationWsUpdate::MembershipDeleted {
            server_ref,
            user_ref,
        } => {
            let Some(mut targets) =
                queries::memberships::get_remote_server_members(
                    &state.db_pool,
                    &server_ref,
                )
                .await?
            else {
                return Ok(());
            };
            match queries::memberships::delete_remote(
                &state.db_pool,
                &server_ref,
                user_ref.clone(),
            )
            .await
            {
                // Not cached here, as for other hosts' users
                Ok(()) | Err(ApiError::NotFound) => {}
                Err(error) => return Err(error),
            }
            if state.config.locality(&user_ref.host).is_local() {
                targets.push(user_ref.clone());
            }
            let _ = state
                .client_ws_manager
                .send_update_to_users(
                    targets,
                    ClientWsUpdate::MembershipDeleted {
                        server_ref,
                        user_ref,
                    },
                )
                .await;
        }

        FederationWsUpdate::ServerUpserted(server) => {
            // Stale or uncached servers are not worth telling anyone about.
            if !queries::servers::update_remote(&state.db_pool, &server).await?
            {
                return Ok(());
            }
            fanout_remote_server_update(
                state,
                &server.to_ref(),
                ClientWsUpdate::ServerUpserted(server),
            )
            .await?;
        }

        FederationWsUpdate::ServerDeleted { server_ref } => {
            fanout_remote_server_update(
                state,
                &server_ref,
                ClientWsUpdate::ServerDeleted {
                    server_ref: server_ref.clone(),
                },
            )
            .await?;
            // Cached memberships go with it.
            queries::servers::delete_remote(&state.db_pool, &server_ref)
                .await?;
        }

        FederationWsUpdate::ChannelUpserted(channel) => {
            fanout_remote_server_update(
                state,
                &channel.server_ref(),
                ClientWsUpdate::ChannelUpserted(channel),
            )
            .await?;
        }

        FederationWsUpdate::ChannelDeleted {
            server_ref,
            channel_id,
        } => {
            fanout_remote_server_update(
                state,
                &server_ref,
                ClientWsUpdate::ChannelDeleted {
                    server_ref: server_ref.clone(),
                    channel_id,
                },
            )
            .await?;
        }

        FederationWsUpdate::MessageUpserted { server_id, message } => {
            fanout_remote_server_update(
                state,
                &ServerRef::new(message.host.clone(), server_id),
                ClientWsUpdate::MessageUpserted(message),
            )
            .await?;
        }

        FederationWsUpdate::RemoteUserDeleted { user_ref } => {
            // Removes the record and its memberships, then tells clients.
            state
                .actors
                .delete_remote_user(user_ref.clone(), Authorization::host())
                .await?;
            state.presence.left(user_ref);
        }

        FederationWsUpdate::PresenceStatusUpdated(status) => {
            state.presence.home_status_updated(status);
        }

        FederationWsUpdate::PresenceStatusesRelayed(relayed) => {
            state.presence.relayed(sender.clone(), relayed);
        }
    }
    Ok(())
}

/// Handle a federation websocket request.
pub(super) async fn handle_federation_request(
    state: &AppState,
    conn_id: ConnId,
    delegated_user_ref: Option<UserRef>,
    request: FederationWsRequest,
) -> ApiResult<FederationWsReply> {
    debug!("WS federation: request={:#?}", request);
    match request {
        FederationWsRequest::ConnectionState => {
            let state = match state
                .federation_ws_manager
                .authenticated_host(conn_id)
                .await
            {
                Some(host) => {
                    FederationWsConnectionState::Authenticated { host }
                }
                None => FederationWsConnectionState::Unauthenticated,
            };
            Ok(FederationWsReply::ConnectionState(state))
        }

        FederationWsRequest::UsersGetAll => {
            let users = ops::users::get_all(state, None).await?;
            Ok(FederationWsReply::UsersGetAll(users))
        }

        FederationWsRequest::UsersGetByRef { user_ref } => {
            // Peers ask a user's home host directly, so this never forwards
            // the request to yet another host.
            if state.config.locality(&user_ref.host).is_remote() {
                return Err(ApiError::NotFound);
            }
            let user = ops::users::get_by_ref(state, user_ref).await?;
            Ok(FederationWsReply::UsersGetByRef(user))
        }

        FederationWsRequest::UsersGetAssociatedHosts { user_ref } => {
            let hosts =
                ops::users::get_associated_hosts(state, user_ref, None).await?;
            Ok(FederationWsReply::UsersGetAssociatedHosts(hosts))
        }

        FederationWsRequest::UsersDelete { user_ref } => {
            let session = authorize_federation(
                state,
                conn_id,
                Some(user_ref.clone()),
                ops::users::auth::federated::delete(user_ref.clone()),
            )
            .await?;
            ops::users::delete_remote_user_record(state, &session).await?;
            state.presence.left(user_ref);
            Ok(FederationWsReply::UsersDelete)
        }

        FederationWsRequest::MembershipsUpsert { new_membership } => {
            let user = new_membership.user.clone();
            if state.config.locality(&user.host).is_local() {
                return Err(ApiError::BadRequest(
                    "User host in membership should not match local host"
                        .into(),
                ));
            }
            // The payload carries the user's profile, which only their home
            // host may provide
            let sender = state
                .federation_ws_manager
                .authenticated_host(conn_id)
                .await;
            if sender.as_ref() != Some(&user.host) {
                return Err(ApiError::AuthError(
                    "Peers may only add memberships for their own users".into(),
                ));
            }
            let new_membership =
                new_membership.with_host(state.config.public_host().clone());
            let mut session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::memberships::auth::federated::create(
                    new_membership.server_ref.clone(),
                    user.as_ref(),
                    new_membership.role,
                ),
            )
            .await?;
            let membership = ops::memberships::upsert(
                state,
                &mut session,
                &new_membership,
                Some(&user),
            )
            .await?;
            Ok(FederationWsReply::MembershipsUpsert(membership))
        }

        FederationWsRequest::MembershipsDelete {
            server_id,
            user_ref,
        } => {
            let server_ref = state.config.local_ref(server_id);
            let mut session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::memberships::auth::federated::delete(
                    server_ref.clone(),
                    user_ref.clone(),
                ),
            )
            .await?;
            ops::memberships::delete(
                state,
                &mut session,
                &server_ref,
                user_ref,
            )
            .await?;
            Ok(FederationWsReply::MembershipsDelete)
        }

        FederationWsRequest::MembershipsGetMembersByServer { server_id } => {
            let members = ops::memberships::get_members_by_server(
                state,
                &state.config.local_ref(server_id),
            )
            .await?;
            Ok(FederationWsReply::MembershipsGetMembersByServer(members))
        }

        FederationWsRequest::MembershipsGetByUserAndServer {
            server_id,
            user_ref,
        } => {
            let member = ops::memberships::get_member_by_user_and_server(
                state,
                &state.config.local_ref(server_id),
                user_ref,
            )
            .await?;
            Ok(FederationWsReply::MembershipsGetByUserAndServer(member))
        }

        FederationWsRequest::ServersCreate(new_server) => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::create(),
            )
            .await?;
            let server =
                ops::servers::create(state, &session, &new_server, None)
                    .await?;
            Ok(FederationWsReply::ServersCreate(server))
        }

        FederationWsRequest::ServersUpdate {
            server_id,
            server_update,
        } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::update(server_ref.clone()),
            )
            .await?;
            let resource = ops::servers::update(
                state,
                &session,
                &server_ref,
                &server_update,
            )
            .await?;
            Ok(FederationWsReply::ServersUpdate(resource))
        }

        FederationWsRequest::ServersDelete { server_id } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::delete(server_ref.clone()),
            )
            .await?;
            ops::servers::delete(state, &session, &server_ref).await?;
            Ok(FederationWsReply::ServersDelete)
        }

        FederationWsRequest::ServersGetAll => {
            let servers = ops::servers::get_all(state, None).await?;
            Ok(FederationWsReply::ServersGetAll(servers))
        }

        FederationWsRequest::ServersGetById { server_id } => {
            let server = ops::servers::get_by_id(
                state,
                &state.config.local_ref(server_id),
            )
            .await?;
            Ok(FederationWsReply::ServersGetById(server))
        }

        FederationWsRequest::ServersGetWithChannels { server_id } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::get_with_channels(
                    server_ref.clone(),
                ),
            )
            .await?;
            let server_with_channels =
                ops::servers::get_with_channels(state, &session, &server_ref)
                    .await?;
            Ok(FederationWsReply::ServersGetWithChannels(
                server_with_channels,
            ))
        }

        FederationWsRequest::ChannelsCreate {
            server_id,
            new_channel,
        } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::create(server_ref.clone()),
            )
            .await?;
            let channel = ops::channels::create(
                state,
                &session,
                &server_ref,
                &new_channel,
            )
            .await?;
            Ok(FederationWsReply::ChannelsCreate(channel))
        }

        FederationWsRequest::ChannelsGetAll => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::get_all(),
            )
            .await?;
            let channels =
                ops::channels::get_all(state, &session, None).await?;
            Ok(FederationWsReply::ChannelsGetAll(channels))
        }

        FederationWsRequest::ChannelsGetByServer { server_id } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::get_by_server(
                    server_ref.clone(),
                ),
            )
            .await?;
            let channels =
                ops::channels::get_by_server(state, &session, &server_ref)
                    .await?;
            Ok(FederationWsReply::ChannelsGetByServer(channels))
        }

        FederationWsRequest::ChannelsGetById { channel_id } => {
            let requirement =
                ops::channels::auth::federated::get_by_id(state, channel_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let channel = ops::channels::get_by_id(
                state,
                &session,
                &state.config.local_ref(channel_id),
            )
            .await?;
            Ok(FederationWsReply::ChannelsGetById(channel))
        }

        FederationWsRequest::ChannelsUpdate {
            channel_id,
            channel_update,
        } => {
            let requirement =
                ops::channels::auth::federated::update(state, channel_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let resource = ops::channels::update(
                state,
                &session,
                &state.config.local_ref(channel_id),
                &channel_update,
            )
            .await?;
            Ok(FederationWsReply::ChannelsUpdate(resource))
        }

        FederationWsRequest::ChannelsDelete { channel_id } => {
            let requirement =
                ops::channels::auth::federated::delete(state, channel_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            ops::channels::delete(
                state,
                &session,
                &state.config.local_ref(channel_id),
            )
            .await?;
            Ok(FederationWsReply::ChannelsDelete)
        }

        FederationWsRequest::ChannelsMove {
            channel_id,
            channel_move,
        } => {
            let requirement =
                ops::channels::auth::federated::move_channel(state, channel_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let channels = ops::channels::move_channel(
                state,
                &session,
                &state.config.local_ref(channel_id),
                &channel_move,
            )
            .await?;
            Ok(FederationWsReply::ChannelsMove(channels))
        }

        FederationWsRequest::MessagesCreate {
            text_channel_id,
            new_message,
        } => {
            let requirement = ops::messages::auth::federated::create(
                state,
                text_channel_id,
                &new_message.author,
            )
            .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let message = ops::messages::create(
                state,
                &session,
                &state.config.local_ref(text_channel_id),
                &new_message,
            )
            .await?;
            Ok(FederationWsReply::MessagesCreate(message))
        }

        FederationWsRequest::MessagesGetAll => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::get_all(),
            )
            .await?;
            let messages =
                ops::messages::get_all(state, &session, None).await?;
            Ok(FederationWsReply::MessagesGetAll(messages))
        }

        FederationWsRequest::MessagesGetByServer { server_id } => {
            let server_ref = state.config.local_ref(server_id);
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::get_by_server(
                    server_ref.clone(),
                ),
            )
            .await?;
            let messages =
                ops::messages::get_by_server(state, &session, &server_ref)
                    .await?;
            Ok(FederationWsReply::MessagesGetByServer(messages))
        }

        FederationWsRequest::MessagesGetByTextChannel { text_channel_id } => {
            let requirement =
                ops::messages::auth::federated::get_by_text_channel(
                    state,
                    text_channel_id,
                )
                .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let messages = ops::messages::get_by_text_channel(
                state,
                &session,
                &state.config.local_ref(text_channel_id),
            )
            .await?;
            Ok(FederationWsReply::MessagesGetByTextChannel(messages))
        }

        FederationWsRequest::MessagesGetById { message_id } => {
            let requirement =
                ops::messages::auth::federated::get_by_id(state, message_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let message = ops::messages::get_by_id(
                state,
                &session,
                &state.config.local_ref(message_id),
            )
            .await?;
            Ok(FederationWsReply::MessagesGetById(message))
        }

        FederationWsRequest::MessagesUpdate {
            message_id,
            message_update,
        } => {
            let location = queries::messages::get_location(
                &state.db_pool,
                state.config.public_host(),
                message_id,
            )
            .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::update(&location),
            )
            .await?;
            let resource = ops::messages::update(
                state,
                &session,
                &message_update,
                ops::messages::MessageTarget::Local(location),
            )
            .await?;
            Ok(FederationWsReply::MessagesUpdate(resource))
        }

        FederationWsRequest::MessagesDelete { message_id } => {
            let location = queries::messages::get_location(
                &state.db_pool,
                state.config.public_host(),
                message_id,
            )
            .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::delete(&location),
            )
            .await?;
            let message = ops::messages::delete(
                state,
                &session,
                ops::messages::MessageTarget::Local(location),
            )
            .await?;
            Ok(FederationWsReply::MessagesDelete(message))
        }

        FederationWsRequest::PresenceStatusesGet { server_ids } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                Requirement::Federation,
            )
            .await?;
            let user_ref = session.user_ref.ok_or_else(|| {
                ApiError::BadRequest(
                    "Presence statuses are only sent for a delegated user"
                        .into(),
                )
            })?;
            let statuses =
                state.presence.statuses(user_ref, server_ids).await?;
            Ok(FederationWsReply::PresenceStatusesGet(statuses))
        }
    }
}
