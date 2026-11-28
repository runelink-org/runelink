use axum::{
    extract::{Json, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::debug;
use runelink_types::{
    host::Host,
    server::{NewServerMembership, ServerId, ServerRef},
    user::UserRef,
};

use crate::{
    auth::{Principal, authorize},
    error::{ApiError, ApiResult},
    ops,
    state::AppState,
};

/// GET /servers/{host}/{id}/users
pub async fn get_members_by_server(
    State(state): State<AppState>,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /servers/{}/{}/users", server_ref.host, server_ref.id);
    let members =
        ops::memberships::get_members_by_server(&state, &server_ref).await?;
    Ok((StatusCode::OK, Json(members)))
}

/// GET /servers/{host}/{id}/users/{user_host}/{name}
pub async fn get_by_user_and_server(
    State(state): State<AppState>,
    Path((host, id, user_host, name)): Path<(Host, ServerId, Host, String)>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /servers/{host}/{id}/users/{user_host}/{name}");
    let member = ops::memberships::get_member_by_user_and_server(
        &state,
        &ServerRef::new(host, id),
        UserRef::new(name, user_host),
    )
    .await?;
    Ok((StatusCode::OK, Json(member)))
}

/// POST /servers/{host}/{id}/users
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
    Json(new_membership): Json<NewServerMembership>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "POST /servers/{}/{}/users\nnew_membership = {:#?}",
        server_ref.host, server_ref.id, new_membership
    );
    if server_ref != new_membership.server_ref {
        return Err(ApiError::BadRequest(
            "Server in path does not match server in membership".into(),
        ));
    }
    let mut session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::memberships::auth::create(&new_membership),
    )
    .await?;
    let membership =
        ops::memberships::upsert(&state, &mut session, &new_membership, None)
            .await?;
    Ok((StatusCode::CREATED, Json(membership)))
}

/// GET /users/{host}/{name}/memberships
pub async fn get_by_user(
    State(state): State<AppState>,
    Path((host, name)): Path<(Host, String)>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /users/{host}/{name}/servers");
    let user_ref = UserRef::new(name, host);
    let memberships = ops::memberships::get_by_user(&state, user_ref).await?;
    Ok((StatusCode::OK, Json(memberships)))
}

/// DELETE /servers/{host}/{id}/users/{user_host}/{name}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((host, id, user_host, name)): Path<(Host, ServerId, Host, String)>,
) -> ApiResult<impl IntoResponse> {
    debug!("DELETE /servers/{host}/{id}/users/{user_host}/{name}");
    let server_ref = ServerRef::new(host, id);
    let user_ref = UserRef::new(name, user_host);
    let mut session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::memberships::auth::delete(&server_ref, user_ref.clone()),
    )
    .await?;
    ops::memberships::delete(&state, &mut session, &server_ref, user_ref)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
