use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::debug;
use runelink_types::{
    host::Host,
    server::{NewServer, ServerRef},
};
use serde::Deserialize;

use crate::{
    auth::{Principal, authorize},
    error::ApiResult,
    ops,
    state::AppState,
};

#[derive(Deserialize, Debug)]
pub struct ServerQueryParams {
    pub target_host: Option<Host>,
}

/// POST /servers
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ServerQueryParams>,
    Json(new_server): Json<NewServer>,
) -> ApiResult<impl IntoResponse> {
    debug!(
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
        params.target_host.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(server)))
}

/// GET /servers
pub async fn get_all(
    State(state): State<AppState>,
    Query(params): Query<ServerQueryParams>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /servers?target_host={:?}", params.target_host);
    let servers =
        ops::servers::get_all(&state, params.target_host.as_ref()).await?;
    Ok((StatusCode::OK, Json(servers)))
}

/// GET /servers/{host}/{id}
pub async fn get_by_id(
    State(state): State<AppState>,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /servers/{}/{}", server_ref.host, server_ref.id);
    let server = ops::servers::get_by_id(&state, &server_ref).await?;
    Ok((StatusCode::OK, Json(server)))
}

/// GET /servers/{host}/{id}/with_channels
pub async fn get_with_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "GET /servers/{}/{}/with_channels",
        server_ref.host, server_ref.id
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::get_with_channels(&server_ref),
    )
    .await?;
    let server_with_channels =
        ops::servers::get_with_channels(&state, &session, &server_ref).await?;
    Ok((StatusCode::OK, Json(server_with_channels)))
}

/// DELETE /servers/{host}/{id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("DELETE /servers/{}/{}", server_ref.host, server_ref.id);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::delete(&server_ref),
    )
    .await?;
    ops::servers::delete(&state, &session, &server_ref).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// PATCH /servers/{host}/{id}
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
    Json(update): Json<runelink_types::server::ServerUpdate>,
) -> ApiResult<impl IntoResponse> {
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::servers::auth::update(&state, &server_ref),
    )
    .await?;
    let resource =
        ops::servers::update(&state, &session, &server_ref, &update).await?;
    Ok((StatusCode::OK, Json(resource)))
}
