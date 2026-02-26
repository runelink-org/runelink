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
use runelink_types::NewServer;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Debug)]
pub struct ServerQueryParams {
    pub target_host: Option<String>,
}

/// POST /servers
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ServerQueryParams>,
    Json(new_server): Json<NewServer>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /servers?target_host={:?}\nnew_server = {:#?}",
        params.target_host, new_server
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::create(),
    )
    .await?;
    let server = ops::servers::create(
        &state,
        &session,
        &new_server,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(server)))
}

/// GET /servers
pub async fn get_all(
    State(state): State<AppState>,
    Query(params): Query<ServerQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!("GET /servers?target_host={:?}", params.target_host);
    let servers =
        ops::servers::get_all(&state, params.target_host.as_deref()).await?;
    Ok((StatusCode::OK, Json(servers)))
}

/// GET /servers/{server_id}
pub async fn get_by_id(
    State(state): State<AppState>,
    Path(server_id): Path<Uuid>,
    Query(params): Query<ServerQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}?target_host={:?}",
        params.target_host
    );
    let server = ops::servers::get_by_id(
        &state,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(server)))
}

/// GET /servers/{server_id}/with_channels
pub async fn get_with_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Query(params): Query<ServerQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/with_channels?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::get_with_channels(server_id),
    )
    .await?;
    let server_with_channels = ops::servers::get_with_channels(
        &state,
        &session,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(server_with_channels)))
}

/// DELETE /servers/{server_id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Query(params): Query<ServerQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "DELETE /servers/{server_id}?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::delete(server_id),
    )
    .await?;
    ops::servers::delete(
        &state,
        &session,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
