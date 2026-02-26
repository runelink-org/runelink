use runelink_types::{
    user::UserRef,
    ws::{FederationWsConnectionState, FederationWsReply, FederationWsRequest},
};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    ops,
    state::AppState,
};

use super::shared::authorize_federation;

pub(super) async fn handle_federation_request(
    state: &AppState,
    conn_id: Uuid,
    delegated_user_ref: Option<UserRef>,
    request: FederationWsRequest,
) -> ApiResult<FederationWsReply> {
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

        FederationWsRequest::MembershipsCreate {
            server_id,
            new_membership,
        } => {
            if server_id != new_membership.server_id {
                return Err(ApiError::BadRequest(
                    "Server ID in path does not match server ID in membership"
                        .into(),
                ));
            }
            if !state
                .config
                .is_remote_host(Some(&new_membership.user_ref.host))
            {
                return Err(ApiError::BadRequest(
                    "User host in membership should not match local host"
                        .into(),
                ));
            }
            let mut session = authorize_federation(
                state,
                conn_id,
                Some(new_membership.user_ref.clone()),
                ops::memberships::auth::federated::create(
                    server_id,
                    new_membership.user_ref.clone(),
                ),
            )
            .await?;
            let membership =
                ops::memberships::create(state, &mut session, &new_membership)
                    .await?;
            Ok(FederationWsReply::MembershipsCreate(membership))
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

        FederationWsRequest::ChannelsGetById {
            server_id,
            channel_id,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::get_by_id(server_id),
            )
            .await?;
            let channel = ops::channels::get_by_id(
                state, &session, server_id, channel_id, None,
            )
            .await?;
            Ok(FederationWsReply::ChannelsGetById(channel))
        }

        FederationWsRequest::ChannelsDelete {
            server_id,
            channel_id,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::channels::auth::federated::delete(server_id),
            )
            .await?;
            ops::channels::delete(state, &session, server_id, channel_id, None)
                .await?;
            Ok(FederationWsReply::ChannelsDelete)
        }

        FederationWsRequest::MessagesCreate {
            server_id,
            channel_id,
            new_message,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::create(server_id),
            )
            .await?;
            let message = ops::messages::create(
                state,
                &session,
                server_id,
                channel_id,
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

        FederationWsRequest::MessagesGetByChannel {
            server_id,
            channel_id,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::get_by_channel(server_id),
            )
            .await?;
            let messages = ops::messages::get_by_channel(
                state, &session, server_id, channel_id, None,
            )
            .await?;
            Ok(FederationWsReply::MessagesGetByChannel(messages))
        }

        FederationWsRequest::MessagesGetById {
            server_id,
            channel_id,
            message_id,
        } => {
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                ops::messages::auth::federated::get_by_id(server_id),
            )
            .await?;
            let message = ops::messages::get_by_id(
                state, &session, server_id, channel_id, message_id, None,
            )
            .await?;
            Ok(FederationWsReply::MessagesGetById(message))
        }

        FederationWsRequest::MessagesDelete {
            server_id,
            channel_id,
            message_id,
        } => {
            let requirement = ops::messages::auth::federated::delete(
                state, server_id, message_id,
            )
            .await?;
            let session = authorize_federation(
                state,
                conn_id,
                delegated_user_ref,
                requirement,
            )
            .await?;
            ops::messages::delete(
                state, &session, server_id, channel_id, message_id, None,
            )
            .await?;
            Ok(FederationWsReply::MessagesDelete)
        }
    }
}
