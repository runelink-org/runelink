use log::info;
use runelink_types::{
    server::ServerId,
    user::UserRef,
    ws::{
        ClientWsUpdate, FederationWsConnectionState, FederationWsReply,
        FederationWsRequest, FederationWsUpdate,
    },
};

use super::shared::authorize_federation;
use crate::{
    config::hosts_match,
    error::{ApiError, ApiResult},
    ids::ConnId,
    ops, queries,
    state::AppState,
};

/// Get the cached members of a remote server, rejecting senders that do not
/// own it. Returns `None` for servers that are not cached.
async fn authorized_remote_server_users(
    state: &AppState,
    sender: &str,
    server_id: ServerId,
) -> ApiResult<Option<Vec<UserRef>>> {
    let Some(members) = state
        .routing_index
        .members_for_remote_server(server_id)
        .await?
    else {
        return Ok(None);
    };
    if !hosts_match(&members.host, sender) {
        return Err(ApiError::Conflict(format!(
            "{sender} does not own server {server_id}"
        )));
    }
    Ok(Some(members.users))
}

/// Fanout a remote server update to the local users (best effort).
async fn fanout_remote_server_update(
    state: &AppState,
    sender: &str,
    server_id: ServerId,
    client_update: ClientWsUpdate,
) -> ApiResult<()> {
    let Some(local_users) =
        authorized_remote_server_users(state, sender, server_id).await?
    else {
        return Ok(());
    };
    let _ = state
        .client_ws_manager
        .send_update_to_users(local_users, client_update)
        .await;
    Ok(())
}

/// Handle a federation websocket update from `sender`, which must own the
/// resource that the update is about.
pub(super) async fn handle_federation_update(
    state: &AppState,
    sender: &str,
    update: FederationWsUpdate,
) -> ApiResult<()> {
    info!("WS federation: update from {sender}: {update:#?}");
    if state.config.locality(sender).is_local() {
        return Err(ApiError::AuthError(
            "Federation updates cannot come from the local host".into(),
        ));
    }
    match update {
        FederationWsUpdate::MembershipUpserted(membership) => {
            queries::servers::upsert_remote(
                &state.db_pool,
                &membership.server,
                sender,
            )
            .await?;
            queries::memberships::insert_remote(
                &state.db_pool,
                &membership.clone().into(),
            )
            .await?;
            fanout_remote_server_update(
                state,
                sender,
                membership.server.id,
                ClientWsUpdate::MembershipUpserted(membership),
            )
            .await?;
        }

        FederationWsUpdate::MembershipDeleted {
            server_id,
            user_ref,
        } => {
            let Some(mut targets) =
                authorized_remote_server_users(state, sender, server_id)
                    .await?
            else {
                return Ok(());
            };
            let _ = queries::memberships::delete_remote(
                &state.db_pool,
                server_id,
                user_ref.clone(),
            )
            .await;
            if state.config.locality(&user_ref.host).is_local() {
                targets.push(user_ref.clone());
            }
            let _ = state
                .client_ws_manager
                .send_update_to_users(
                    targets,
                    ClientWsUpdate::MembershipDeleted {
                        server_id,
                        user_ref,
                    },
                )
                .await;
        }

        FederationWsUpdate::ServerUpserted(server) => {
            queries::servers::upsert_remote(&state.db_pool, &server, sender)
                .await?;
            fanout_remote_server_update(
                state,
                sender,
                server.id,
                ClientWsUpdate::ServerUpserted(server),
            )
            .await?;
        }

        FederationWsUpdate::ServerDeleted { server_id } => {
            fanout_remote_server_update(
                state,
                sender,
                server_id,
                ClientWsUpdate::ServerDeleted { server_id },
            )
            .await?;
        }

        FederationWsUpdate::ChannelUpserted(channel) => {
            fanout_remote_server_update(
                state,
                sender,
                channel.server_id,
                ClientWsUpdate::ChannelUpserted(channel),
            )
            .await?;
        }

        FederationWsUpdate::ChannelDeleted {
            server_id,
            channel_id,
        } => {
            fanout_remote_server_update(
                state,
                sender,
                server_id,
                ClientWsUpdate::ChannelDeleted {
                    server_id,
                    channel_id,
                },
            )
            .await?;
        }

        FederationWsUpdate::MessageUpserted { server_id, message } => {
            fanout_remote_server_update(
                state,
                sender,
                server_id,
                ClientWsUpdate::MessageUpserted(message),
            )
            .await?;
        }

        FederationWsUpdate::RemoteUserDeleted { user_ref } => {
            if !hosts_match(&user_ref.host, sender) {
                return Err(ApiError::Conflict(format!(
                    "{sender} cannot delete users on {}",
                    user_ref.host
                )));
            }
            let _ = state
                .client_ws_manager
                .broadcast_update(ClientWsUpdate::UserDeleted { user_ref })
                .await;
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
    info!("WS federation: request={:#?}", request);
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
            let user = ops::users::get_by_ref(state, user_ref, None).await?;
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
            ops::users::delete_remote_user_record(state, &session, &user_ref)
                .await?;
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
            let mut session = authorize_federation(
                state,
                conn_id,
                Some(user.as_ref()),
                ops::memberships::auth::federated::create(
                    new_membership.server_id,
                    user.as_ref(),
                ),
            )
            .await?;
            let membership = ops::memberships::upsert(
                state,
                &mut session,
                &new_membership.into(),
                Some(&user),
            )
            .await?;
            Ok(FederationWsReply::MembershipsUpsert(membership))
        }

        FederationWsRequest::MembershipsGetByUser { user_ref } => {
            let memberships =
                ops::memberships::get_by_user(state, user_ref).await?;
            Ok(FederationWsReply::MembershipsGetByUser(memberships))
        }

        FederationWsRequest::MembershipsDelete {
            server_id,
            user_ref,
        } => {
            let mut session = authorize_federation(
                state,
                conn_id,
                Some(user_ref.clone()),
                ops::memberships::auth::federated::delete(
                    server_id,
                    user_ref.clone(),
                ),
            )
            .await?;
            ops::memberships::delete(
                state,
                &mut session,
                server_id,
                user_ref,
                None,
            )
            .await?;
            Ok(FederationWsReply::MembershipsDelete)
        }

        FederationWsRequest::MembershipsGetMembersByServer { server_id } => {
            let members =
                ops::memberships::get_members_by_server(state, server_id, None)
                    .await?;
            Ok(FederationWsReply::MembershipsGetMembersByServer(members))
        }

        FederationWsRequest::MembershipsGetByUserAndServer {
            server_id,
            user_ref,
        } => {
            let member = ops::memberships::get_member_by_user_and_server(
                state, server_id, user_ref, None,
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
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::update(state, server_id).await?,
            )
            .await?;
            let resource = ops::servers::update(
                state,
                &session,
                server_id,
                &server_update,
                None,
            )
            .await?;
            Ok(FederationWsReply::ServersUpdate(resource))
        }
        FederationWsRequest::ServersDelete { server_id } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::delete(server_id),
            )
            .await?;
            ops::servers::delete(state, &session, server_id, None).await?;
            Ok(FederationWsReply::ServersDelete)
        }

        FederationWsRequest::ServersGetAll => {
            let servers = ops::servers::get_all(state, None).await?;
            Ok(FederationWsReply::ServersGetAll(servers))
        }

        FederationWsRequest::ServersGetById { server_id } => {
            let server =
                ops::servers::get_by_id(state, server_id, None).await?;
            Ok(FederationWsReply::ServersGetById(server))
        }

        FederationWsRequest::ServersGetWithChannels { server_id } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::servers::auth::federated::get_with_channels(server_id),
            )
            .await?;
            let server_with_channels = ops::servers::get_with_channels(
                state, &session, server_id, None,
            )
            .await?;
            Ok(FederationWsReply::ServersGetWithChannels(
                server_with_channels,
            ))
        }

        FederationWsRequest::ChannelsCreate {
            server_id,
            new_channel,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::create(server_id),
            )
            .await?;
            let channel = ops::channels::create(
                state,
                &session,
                server_id,
                &new_channel,
                None,
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
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::get_by_server(server_id),
            )
            .await?;
            let channels =
                ops::channels::get_by_server(state, &session, server_id, None)
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
            let channel =
                ops::channels::get_by_id(state, &session, channel_id, None)
                    .await?;
            Ok(FederationWsReply::ChannelsGetById(channel))
        }

        FederationWsRequest::ChannelsUpdate {
            channel_id,
            channel_update,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::update(state, channel_id)
                    .await?,
            )
            .await?;
            let resource = ops::channels::update(
                state,
                &session,
                channel_id,
                &channel_update,
                None,
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
            ops::channels::delete(state, &session, channel_id, None).await?;
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
                channel_id,
                &channel_move,
                None,
            )
            .await?;
            Ok(FederationWsReply::ChannelsMove(channels))
        }

        FederationWsRequest::MessagesCreate {
            text_channel_id,
            new_message,
        } => {
            let requirement =
                ops::messages::auth::federated::create(state, text_channel_id)
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
                text_channel_id,
                &new_message,
                None,
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
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::get_by_server(server_id),
            )
            .await?;
            let messages =
                ops::messages::get_by_server(state, &session, server_id, None)
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
                text_channel_id,
                None,
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
            let message =
                ops::messages::get_by_id(state, &session, message_id, None)
                    .await?;
            Ok(FederationWsReply::MessagesGetById(message))
        }

        FederationWsRequest::MessagesUpdate {
            message_id,
            message_update,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::update(state, message_id)
                    .await?,
            )
            .await?;
            let resource = ops::messages::update(
                state,
                &session,
                message_id,
                &message_update,
                None,
            )
            .await?;
            Ok(FederationWsReply::MessagesUpdate(resource))
        }
        FederationWsRequest::MessagesDelete { message_id } => {
            let requirement =
                ops::messages::auth::federated::delete(state, message_id)
                    .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            let message =
                ops::messages::delete(state, &session, message_id, None)
                    .await?;
            Ok(FederationWsReply::MessagesDelete(message))
        }
    }
}
