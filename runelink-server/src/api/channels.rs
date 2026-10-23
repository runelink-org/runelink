use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::info;
use runelink_types::{
    channel::{NewServerChannel, ServerChannelId, ServerChannelMove},
    server::ServerId,
};
use serde::Deserialize;

use crate::{
    auth::{Principal, authorize},
    error::ApiResult,
    ops,
    state::AppState,
};

#[derive(Deserialize, Debug)]
pub struct ChannelQueryParams {
    pub target_host: Option<String>,
}

/// POST /servers/{server_id}/channels
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<ServerId>,
    Query(params): Query<ChannelQueryParams>,
    Json(new_channel): Json<NewServerChannel>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /servers/{server_id}/channels?target_host={:?}\nnew_channel = {:#?}",
        params.target_host, new_channel,
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::create(server_id),
    )
    .await?;
    let channel = ops::channels::create(
        &state,
        &session,
        server_id,
        &new_channel,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(channel)))
}

/// GET /channels
pub async fn get_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ChannelQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!("GET /channels?target_host={:?}", params.target_host);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_all(),
    )
    .await?;
    let channels =
        ops::channels::get_all(&state, &session, params.target_host.as_deref())
            .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// GET /servers/{server_id}/channels
pub async fn get_by_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<ServerId>,
    Query(params): Query<ChannelQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /servers/{server_id}/channels?target_host={:?}",
        params.target_host
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_by_server(server_id),
    )
    .await?;
    let channels = ops::channels::get_by_server(
        &state,
        &session,
        server_id,
        params.target_host.as_deref(),
    )
    .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// GET /channels/{channel_id}
pub async fn get_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<ServerChannelId>,
    Query(params): Query<ChannelQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /channels/{channel_id}?target_host={:?}",
        params.target_host
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_by_id(&state, channel_id, target_host).await?,
    )
    .await?;
    let channel =
        ops::channels::get_by_id(&state, &session, channel_id, target_host)
            .await?;
    Ok((StatusCode::OK, Json(channel)))
}

/// DELETE /channels/{channel_id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<ServerChannelId>,
    Query(params): Query<ChannelQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "DELETE /channels/{channel_id}?target_host={:?}",
        params.target_host
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::delete(&state, channel_id, target_host).await?,
    )
    .await?;
    ops::channels::delete(&state, &session, channel_id, target_host).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /channels/{channel_id}/move
pub async fn move_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<ServerChannelId>,
    Query(params): Query<ChannelQueryParams>,
    Json(channel_move): Json<ServerChannelMove>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /channels/{channel_id}/move?target_host={:?}\nchannel_move = {:#?}",
        params.target_host, channel_move,
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::move_channel(&state, channel_id, target_host)
            .await?,
    )
    .await?;
    let channels = ops::channels::move_channel(
        &state,
        &session,
        channel_id,
        &channel_move,
        target_host,
    )
    .await?;
    Ok((StatusCode::OK, Json(channels)))
}
