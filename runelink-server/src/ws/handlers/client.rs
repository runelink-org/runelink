use jsonwebtoken::{Algorithm, Validation};
use log::info;
use runelink_types::{
    auth::{ClientAccessClaims, JwksResponse, OidcDiscoveryDocument},
    user::UserRef,
    ws::{
        AuthTokenAccessRequest, ClientWsConnectionState, ClientWsReply,
        ClientWsRequest,
    },
};
use uuid::Uuid;

use crate::{
    bearer_auth::ClientAuth,
    error::{ApiError, ApiResult},
    ops,
    state::AppState,
};

use super::shared::authorize_client;

/// Extract the client authentication from an access token.
fn client_auth_from_access_token(
    state: &AppState,
    access_token: &str,
) -> ApiResult<ClientAuth> {
    let server_id = state.config.api_url();
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(std::slice::from_ref(&server_id));
    validation.set_issuer(std::slice::from_ref(&server_id));

    let data = jsonwebtoken::decode::<ClientAccessClaims>(
        access_token,
        &state.key_manager.decoding_key,
        &validation,
    )
    .map_err(|_| ApiError::AuthError("Invalid or expired token".into()))?;

    Ok(ClientAuth {
        claims: data.claims,
    })
}

/// Handle a client websocket request.
pub(super) async fn handle_client_request(
    state: &AppState,
    conn_id: Uuid,
    request: ClientWsRequest,
) -> ApiResult<ClientWsReply> {
    info!("WS client: request={:#?}", request);
    match request {
        ClientWsRequest::Ping => Ok(ClientWsReply::Pong),

        ClientWsRequest::OidcDiscovery => {
            let issuer = state.config.api_url();
            Ok(ClientWsReply::OidcDiscovery(OidcDiscoveryDocument {
                issuer: issuer.clone(),
                jwks_uri: format!("{issuer}/.well-known/jwks.json"),
                token_endpoint: format!("{issuer}/auth/token"),
                userinfo_endpoint: format!("{issuer}/auth/userinfo"),
                grant_types_supported: vec![
                    "password".into(),
                    "refresh_token".into(),
                ],
                response_types_supported: vec![],
                scopes_supported: vec![],
                token_endpoint_auth_methods_supported: vec!["none".into()],
            }))
        }

        ClientWsRequest::OidcJwks => {
            Ok(ClientWsReply::OidcJwks(JwksResponse {
                keys: vec![state.key_manager.public_jwk.clone()],
            }))
        }

        ClientWsRequest::ConnectionState => {
            let state = match state
                .client_ws_manager
                .authenticated_user_ref(conn_id)
                .await
            {
                Some(user_ref) => {
                    ClientWsConnectionState::Authenticated { user_ref }
                }
                None => ClientWsConnectionState::Unauthenticated,
            };
            Ok(ClientWsReply::ConnectionState(state))
        }

        ClientWsRequest::AuthTokenAccess(AuthTokenAccessRequest {
            access_token,
        }) => {
            let auth = client_auth_from_access_token(state, &access_token)?;
            let user_ref = UserRef::parse_subject(&auth.claims.sub)
                .ok_or_else(|| {
                    ApiError::AuthError(
                        "Invalid token subject (expected name@host)".into(),
                    )
                })?;
            let authenticated = state
                .client_ws_manager
                .authenticate_connection(conn_id, user_ref.clone())
                .await;
            if !authenticated {
                return Err(ApiError::Internal(
                    "Client websocket connection not registered".into(),
                ));
            }
            Ok(ClientWsReply::AuthTokenAccess(
                ClientWsConnectionState::Authenticated { user_ref },
            ))
        }

        ClientWsRequest::AuthSignup(_)
        | ClientWsRequest::AuthTokenPassword(_)
        | ClientWsRequest::AuthTokenRefresh(_)
        | ClientWsRequest::AuthUserinfo
        | ClientWsRequest::AuthRegisterClient => Err(ApiError::BadRequest(
            "This auth operation is not implemented over websocket".into(),
        )),

        ClientWsRequest::UsersCreate(new_user) => {
            let session =
                authorize_client(state, conn_id, ops::users::auth::create())
                    .await?;
            let user = ops::users::create(state, &session, &new_user).await?;
            Ok(ClientWsReply::UsersCreate(user))
        }

        ClientWsRequest::UsersGetAll { target_host } => {
            let users =
                ops::users::get_all(state, target_host.as_deref()).await?;
            Ok(ClientWsReply::UsersGetAll(users))
        }

        ClientWsRequest::UsersGetByRef {
            user_ref,
            target_host,
        } => {
            let user =
                ops::users::get_by_ref(state, user_ref, target_host.as_deref())
                    .await?;
            Ok(ClientWsReply::UsersGetByRef(user))
        }

        ClientWsRequest::UsersGetAssociatedHosts {
            user_ref,
            target_host,
        } => {
            let hosts = ops::users::get_associated_hosts(
                state,
                user_ref,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::UsersGetAssociatedHosts(hosts))
        }

        ClientWsRequest::UsersDelete { user_ref } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::users::auth::delete(user_ref.clone()),
            )
            .await?;
            ops::users::delete_home_user(state, &session, &user_ref).await?;
            Ok(ClientWsReply::UsersDelete)
        }

        ClientWsRequest::MembershipsGetByUser { user_ref } => {
            let memberships =
                ops::memberships::get_by_user(state, user_ref).await?;
            Ok(ClientWsReply::MembershipsGetByUser(memberships))
        }

        ClientWsRequest::MembershipsGetMembersByServer {
            server_id,
            target_host,
        } => {
            let members = ops::memberships::get_members_by_server(
                state,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MembershipsGetMembersByServer(members))
        }

        ClientWsRequest::MembershipsGetByUserAndServer {
            server_id,
            user_ref,
            target_host,
        } => {
            let member = ops::memberships::get_member_by_user_and_server(
                state,
                server_id,
                user_ref,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MembershipsGetByUserAndServer(member))
        }

        ClientWsRequest::MembershipsCreate {
            server_id,
            new_membership,
        } => {
            if server_id != new_membership.server_id {
                return Err(ApiError::BadRequest(
                    "Server ID in path does not match server ID in membership"
                        .into(),
                ));
            }
            let mut session = authorize_client(
                state,
                conn_id,
                ops::memberships::auth::create(server_id),
            )
            .await?;
            let membership =
                ops::memberships::create(state, &mut session, &new_membership)
                    .await?;
            Ok(ClientWsReply::MembershipsCreate(membership))
        }

        ClientWsRequest::MembershipsDelete {
            server_id,
            user_ref,
            target_host,
        } => {
            let mut session = authorize_client(
                state,
                conn_id,
                ops::memberships::auth::delete(server_id, user_ref.clone()),
            )
            .await?;
            ops::memberships::delete(
                state,
                &mut session,
                server_id,
                user_ref,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MembershipsDelete)
        }

        ClientWsRequest::ServersCreate {
            new_server,
            target_host,
        } => {
            let session =
                authorize_client(state, conn_id, ops::servers::auth::create())
                    .await?;
            let server = ops::servers::create(
                state,
                &session,
                &new_server,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ServersCreate(server))
        }

        ClientWsRequest::ServersGetAll { target_host } => {
            let servers =
                ops::servers::get_all(state, target_host.as_deref()).await?;
            Ok(ClientWsReply::ServersGetAll(servers))
        }

        ClientWsRequest::ServersGetById {
            server_id,
            target_host,
        } => {
            let server = ops::servers::get_by_id(
                state,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ServersGetById(server))
        }

        ClientWsRequest::ServersGetWithChannels {
            server_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::servers::auth::get_with_channels(server_id),
            )
            .await?;
            let server_with_channels = ops::servers::get_with_channels(
                state,
                &session,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ServersGetWithChannels(server_with_channels))
        }

        ClientWsRequest::ServersDelete {
            server_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::servers::auth::delete(server_id),
            )
            .await?;
            ops::servers::delete(
                state,
                &session,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ServersDelete)
        }

        ClientWsRequest::ChannelsCreate {
            server_id,
            new_channel,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::channels::auth::create(server_id),
            )
            .await?;
            let channel = ops::channels::create(
                state,
                &session,
                server_id,
                &new_channel,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ChannelsCreate(channel))
        }

        ClientWsRequest::ChannelsGetAll { target_host } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::channels::auth::get_all(),
            )
            .await?;
            let channels =
                ops::channels::get_all(state, &session, target_host.as_deref())
                    .await?;
            Ok(ClientWsReply::ChannelsGetAll(channels))
        }

        ClientWsRequest::ChannelsGetByServer {
            server_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::channels::auth::get_by_server(server_id),
            )
            .await?;
            let channels = ops::channels::get_by_server(
                state,
                &session,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ChannelsGetByServer(channels))
        }

        ClientWsRequest::ChannelsGetById {
            server_id,
            channel_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::channels::auth::get_by_id(server_id),
            )
            .await?;
            let channel = ops::channels::get_by_id(
                state,
                &session,
                server_id,
                channel_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ChannelsGetById(channel))
        }

        ClientWsRequest::ChannelsDelete {
            server_id,
            channel_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::channels::auth::delete(server_id),
            )
            .await?;
            ops::channels::delete(
                state,
                &session,
                server_id,
                channel_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::ChannelsDelete)
        }

        ClientWsRequest::MessagesCreate {
            server_id,
            channel_id,
            new_message,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::messages::auth::create(server_id),
            )
            .await?;
            let message = ops::messages::create(
                state,
                &session,
                server_id,
                channel_id,
                &new_message,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MessagesCreate(message))
        }

        ClientWsRequest::MessagesGetAll { target_host } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::messages::auth::get_all(),
            )
            .await?;
            let messages =
                ops::messages::get_all(state, &session, target_host.as_deref())
                    .await?;
            Ok(ClientWsReply::MessagesGetAll(messages))
        }

        ClientWsRequest::MessagesGetByServer {
            server_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::messages::auth::get_by_server(server_id),
            )
            .await?;
            let messages = ops::messages::get_by_server(
                state,
                &session,
                server_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MessagesGetByServer(messages))
        }

        ClientWsRequest::MessagesGetByChannel {
            server_id,
            channel_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::messages::auth::get_by_channel(server_id),
            )
            .await?;
            let messages = ops::messages::get_by_channel(
                state,
                &session,
                server_id,
                channel_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MessagesGetByChannel(messages))
        }

        ClientWsRequest::MessagesGetById {
            server_id,
            channel_id,
            message_id,
            target_host,
        } => {
            let session = authorize_client(
                state,
                conn_id,
                ops::messages::auth::get_by_id(server_id),
            )
            .await?;
            let message = ops::messages::get_by_id(
                state,
                &session,
                server_id,
                channel_id,
                message_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MessagesGetById(message))
        }

        ClientWsRequest::MessagesDelete {
            server_id,
            channel_id,
            message_id,
            target_host,
        } => {
            let requirement =
                ops::messages::auth::delete(state, server_id, message_id)
                    .await?;
            let session = authorize_client(state, conn_id, requirement).await?;
            ops::messages::delete(
                state,
                &session,
                server_id,
                channel_id,
                message_id,
                target_host.as_deref(),
            )
            .await?;
            Ok(ClientWsReply::MessagesDelete)
        }
    }
}
