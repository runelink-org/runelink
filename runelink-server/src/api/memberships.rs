use crate::{
    auth::{Principal, authorize},
    error::{ApiError, ApiResult},
    ops,
    state::AppState,
};
use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::info;
use runelink_types::{NewServerMembership, UserRef};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Debug)]
pub struct MembershipQueryParams {
    pub target_host: Option<String>,
}

/// GET /servers/{server_id}/users
pub async fn get_members_by_server(
    State(state): State<AppState>,
    Path(server_id): Path<Uuid>,
    Query(params): Query<MembershipQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/users?target_host={:?}",
        params.target_host
    );
    let members = ops::memberships::get_members_by_server(
        &state,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(members)))
}

/// GET /servers/{server_id}/users/{host}/{name}
pub async fn get_by_user_and_server(
    State(state): State<AppState>,
    Path((server_id, host, name)): Path<(Uuid, String, String)>,
    Query(params): Query<MembershipQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/users/{host}/{name}?target_host={:?}",
        params.target_host
    );
    let member = ops::memberships::get_member_by_user_and_server(
        &state,
        server_id,
        UserRef::new(name, host),
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(member)))
}

/// POST /servers/{server_id}/users
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Json(new_membership): Json<NewServerMembership>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /servers/{server_id}/users\nnew_membership = {:#?}",
        new_membership
    );
    if server_id != new_membership.server_id {
        return Err(ApiError::BadRequest(
            "Server ID in path does not match server ID in membership".into(),
        ));
    }
    let mut session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::memberships::auth::create(server_id),
    )
    .await?;
    let membership =
        ops::memberships::create(&state, &mut session, &new_membership).await?;
    Ok((StatusCode::CREATED, Json(membership)))
}

/// GET /users/{host}/{name}/memberships
pub async fn get_by_user(
    State(state): State<AppState>,
    Path((host, name)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    info!("GET /users/{host}/{name}/servers");
    let user_ref = UserRef::new(name, host);
    let memberships = ops::memberships::get_by_user(&state, user_ref).await?;
    Ok((StatusCode::OK, Json(memberships)))
}

/// DELETE /servers/{server_id}/users/{host}/{name}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, host, name)): Path<(Uuid, String, String)>,
    Query(params): Query<MembershipQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "DELETE /servers/{server_id}/users/{host}/{name}?target_host={:?}",
        params.target_host
    );
    let user_ref = UserRef::new(name, host);
    let mut session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::memberships::auth::delete(server_id, user_ref.clone()),
    )
    .await?;
    ops::memberships::delete(
        &state,
        &mut session,
        server_id,
        user_ref,
        params.target_host.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
