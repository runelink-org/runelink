use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::debug;
use runelink_types::{
    channel::{NewServerChannel, ServerChannelMove, ServerChannelRef},
    host::Host,
    server::ServerRef,
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
    pub target_host: Option<Host>,
}

/// POST /servers/{host}/{id}/channels
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
    Json(new_channel): Json<NewServerChannel>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "POST /servers/{}/{}/channels\nnew_channel = {:#?}",
        server_ref.host, server_ref.id, new_channel,
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::create(&server_ref),
    )
    .await?;
    let channel =
        ops::channels::create(&state, &session, &server_ref, &new_channel)
            .await?;
    Ok((StatusCode::CREATED, Json(channel)))
}

/// GET /channels
pub async fn get_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ChannelQueryParams>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /channels?target_host={:?}", params.target_host);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_all(),
    )
    .await?;
    let channels =
        ops::channels::get_all(&state, &session, params.target_host.as_ref())
            .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// GET /servers/{host}/{id}/channels
pub async fn get_by_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "GET /servers/{}/{}/channels",
        server_ref.host, server_ref.id
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_by_server(&server_ref),
    )
    .await?;
    let channels =
        ops::channels::get_by_server(&state, &session, &server_ref).await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// GET /channels/{host}/{id}
pub async fn get_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_ref): Path<ServerChannelRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /channels/{}/{}", channel_ref.host, channel_ref.id);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::get_by_id(&state, &channel_ref).await?,
    )
    .await?;
    let channel =
        ops::channels::get_by_id(&state, &session, &channel_ref).await?;
    Ok((StatusCode::OK, Json(channel)))
}

/// DELETE /channels/{host}/{id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_ref): Path<ServerChannelRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("DELETE /channels/{}/{}", channel_ref.host, channel_ref.id);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::delete(&state, &channel_ref).await?,
    )
    .await?;
    ops::channels::delete(&state, &session, &channel_ref).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /channels/{host}/{id}/move
pub async fn move_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_ref): Path<ServerChannelRef>,
    Json(channel_move): Json<ServerChannelMove>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "POST /channels/{}/{}/move\nchannel_move = {:#?}",
        channel_ref.host, channel_ref.id, channel_move
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::move_channel(&state, &channel_ref).await?,
    )
    .await?;
    let channels = ops::channels::move_channel(
        &state,
        &session,
        &channel_ref,
        &channel_move,
    )
    .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// PATCH /channels/{host}/{id}
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_ref): Path<ServerChannelRef>,
    Json(update): Json<runelink_types::channel::ServerChannelUpdate>,
) -> ApiResult<impl IntoResponse> {
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::channels::auth::update(&state, &channel_ref).await?,
    )
    .await?;
    let resource =
        ops::channels::update(&state, &session, &channel_ref, &update).await?;
    Ok((StatusCode::OK, Json(resource)))
}
