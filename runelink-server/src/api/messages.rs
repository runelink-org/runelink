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
use runelink_types::NewMessage;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Debug)]
pub struct MessageQueryParams {
    pub target_host: Option<String>,
}

/// POST /servers/{server_id}/channels/{channel_id}/messages
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<MessageQueryParams>,
    Json(new_message): Json<NewMessage>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /servers/{server_id}/channels/{channel_id}/messages?target_host={:?}\nnew_message = {:#?}",
        params.target_host, new_message
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::create(server_id),
    )
    .await?;
    let message = ops::messages::create(
        &state,
        &session,
        server_id,
        channel_id,
        &new_message,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(message)))
}

/// GET /messages
pub async fn get_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!("GET /messages?target_host={:?}", params.target_host);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_all(),
    )
    .await?;
    let messages = ops::messages::get_all(
        &state,
        &session,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /servers/{server_id}/messages
pub async fn get_by_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/messages?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_server(server_id),
    )
    .await?;
    let messages = ops::messages::get_by_server(
        &state,
        &session,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /servers/{server_id}/channels/{channel_id}/messages
pub async fn get_by_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/channels/{channel_id}/messages?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_channel(server_id),
    )
    .await?;
    let messages = ops::messages::get_by_channel(
        &state,
        &session,
        server_id,
        channel_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /servers/{server_id}/channels/{channel_id}/messages/{message_id}
pub async fn get_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, channel_id, message_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/channels/{channel_id}/messages/{message_id}?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_id(server_id),
    )
    .await?;
    let message = ops::messages::get_by_id(
        &state,
        &session,
        server_id,
        channel_id,
        message_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(message)))
}

/// DELETE /servers/{server_id}/channels/{channel_id}/messages/{message_id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((server_id, channel_id, message_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "DELETE /servers/{server_id}/channels/{channel_id}/messages/{message_id}?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::delete(&state, server_id, message_id).await?,
    )
    .await?;
    ops::messages::delete(
        &state,
        &session,
        server_id,
        channel_id,
        message_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
