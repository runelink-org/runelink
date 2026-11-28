use runelink_types::{ClientAccessClaims, UserRef};
use time::Duration;

use crate::{
    auth::{Principal, Requirement, Session, authorize},
    bearer_auth::{ClientAuth, FederationAuth},
    error::{ApiError, ApiResult},
    ids::ConnId,
    state::AppState,
};

pub(super) async fn authorize_client(
    state: &AppState,
    conn_id: ConnId,
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
    conn_id: ConnId,
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
    let auth = FederationAuth::new(host, delegated_user_ref)?;
    authorize(state, Principal::Federation(auth), requirement).await
}
