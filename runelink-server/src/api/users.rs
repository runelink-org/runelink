use crate::{
    auth::{Principal, authorize},
    error::ApiResult,
    ops,
    state::AppState,
};
use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::info;
use runelink_types::{NewUser, UserRef};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct UserQueryParams {
    pub target_host: Option<String>,
}

/// POST /users
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new_user): Json<NewUser>,
) -> ApiResult<impl IntoResponse> {
    info!("POST /users\nnew_user = {:#?}", new_user);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::users::auth::create(),
    )
    .await?;
    let user = ops::users::create(&state, &session, &new_user).await?;
    Ok((StatusCode::CREATED, Json(user)))
}

/// GET /users
pub async fn get_all(
    State(state): State<AppState>,
    Query(params): Query<UserQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!("GET /users?target_host={:?}", params.target_host);
    let users =
        ops::users::get_all(&state, params.target_host.as_deref()).await?;
    Ok((StatusCode::OK, Json(users)))
}

/// GET /users/{host}/{name}
pub async fn get_by_ref(
    State(state): State<AppState>,
    Path((host, name)): Path<(String, String)>,
    Query(params): Query<UserQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /users/{host}/{name}?target_host={:?}",
        params.target_host
    );
    let user_ref = UserRef::new(name, host);
    let user = ops::users::get_by_ref(
        &state,
        user_ref,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(user)))
}

/// GET /users/{host}/{name}/hosts
pub async fn get_associated_hosts(
    State(state): State<AppState>,
    Path((host, name)): Path<(String, String)>,
    Query(params): Query<UserQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /users/{host}/{name}/hosts?target_host={:?}",
        params.target_host
    );
    let user_ref = UserRef::new(name, host);
    let hosts = ops::users::get_associated_hosts(
        &state,
        user_ref,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(hosts)))
}

/// DELETE /users/{host}/{name}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((host, name)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let user_ref = UserRef::new(name.clone(), host.clone());
    info!("DELETE /users/{host}/{name}");
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::users::auth::delete(user_ref.clone()),
    )
    .await?;
    ops::users::delete_home_user(&state, &session, &user_ref).await?;
    Ok(StatusCode::NO_CONTENT)
}
