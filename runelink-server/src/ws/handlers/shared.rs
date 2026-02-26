use runelink_client::util::get_api_url;
use runelink_types::{ClientAccessClaims, FederationClaims, UserRef};
use time::Duration;
use uuid::Uuid;

use crate::{
    auth::{Principal, Requirement, Session, authorize},
    bearer_auth::{ClientAuth, FederationAuth},
    error::{ApiError, ApiResult},
    state::AppState,
};

pub(super) async fn authorize_client(
    state: &AppState,
    conn_id: Uuid,
    requirement: Requirement,
) -> ApiResult<Session> {
    let user_ref = state
        .client_ws_manager
        .authenticated_user_ref(conn_id)
        .await
        .ok_or_else(|| {
            ApiError::AuthError(
                "Client websocket connection is not authenticated".into(),
            )
        })?;

    let claims = ClientAccessClaims::new(
        &user_ref,
        "ws".into(),
        state.config.api_url(),
        "openid".into(),
        Duration::hours(1),
    );
    let principal = Principal::Client(ClientAuth { claims });
    authorize(state, principal, requirement).await
}

pub(super) async fn authorize_federation(
    state: &AppState,
    conn_id: Uuid,
    delegated_user_ref: Option<UserRef>,
    requirement: Requirement,
) -> ApiResult<Session> {
    let host = state
        .federation_ws_manager
        .authenticated_host(conn_id)
        .await
        .ok_or_else(|| {
            ApiError::AuthError(
                "Federation websocket connection is not authenticated".into(),
            )
        })?;

    let issuer = get_api_url(&host);
    let claims = match delegated_user_ref {
        Some(user_ref) => FederationClaims::new_delegated(
            issuer,
            state.config.api_url(),
            user_ref,
            Duration::hours(1),
        ),
        None => FederationClaims::new_server_only(
            issuer,
            state.config.api_url(),
            Duration::hours(1),
        ),
    };

    let principal = Principal::Federation(FederationAuth { claims });
    authorize(state, principal, requirement).await
}
