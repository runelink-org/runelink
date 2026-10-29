use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::info;
use runelink_types::{
    channel::TextChannelId,
    message::{MessageId, NewMessage},
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
pub struct MessageQueryParams {
    pub target_host: Option<String>,
}

/// POST /text_channels/{text_channel_id}/messages
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(text_channel_id): Path<TextChannelId>,
    Query(params): Query<MessageQueryParams>,
    Json(new_message): Json<NewMessage>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "POST /text_channels/{text_channel_id}/messages?target_host={:?}\nnew_message = {:#?}",
        params.target_host, new_message
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::create(&state, text_channel_id, target_host)
            .await?,
    )
    .await?;
    let message = ops::messages::create(
        &state,
        &session,
        text_channel_id,
        &new_message,
        target_host,
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
    let messages =
        ops::messages::get_all(&state, &session, params.target_host.as_deref())
            .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /servers/{server_id}/messages
pub async fn get_by_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<ServerId>,
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

/// GET /text_channels/{text_channel_id}/messages
pub async fn get_by_text_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(text_channel_id): Path<TextChannelId>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /text_channels/{text_channel_id}/messages?target_host={:?}",
        params.target_host
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_text_channel(
            &state,
            text_channel_id,
            target_host,
        )
        .await?,
    )
    .await?;
    let messages = ops::messages::get_by_text_channel(
        &state,
        &session,
        text_channel_id,
        target_host,
    )
    .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /messages/{message_id}
pub async fn get_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<MessageId>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "GET /messages/{message_id}?target_host={:?}",
        params.target_host
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_id(&state, message_id, target_host).await?,
    )
    .await?;
    let message =
        ops::messages::get_by_id(&state, &session, message_id, target_host)
            .await?;
    Ok((StatusCode::OK, Json(message)))
}

/// DELETE /messages/{message_id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<MessageId>,
    Query(params): Query<MessageQueryParams>,
) -> ApiResult<impl IntoResponse> {
    info!(
        "DELETE /messages/{message_id}?target_host={:?}",
        params.target_host
    );
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::delete(&state, message_id, target_host).await?,
    )
    .await?;
    let message =
        ops::messages::delete(&state, &session, message_id, target_host)
            .await?;
    Ok((StatusCode::OK, Json(message)))
}

/// PATCH /messages/{message_id}
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<MessageId>,
    Query(params): Query<MessageQueryParams>,
    Json(update): Json<runelink_types::message::MessageUpdate>,
) -> ApiResult<impl IntoResponse> {
    let target_host = params.target_host.as_deref();
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::update(&state, message_id, target_host).await?,
    )
    .await?;
    let resource = ops::messages::update(
        &state,
        &session,
        message_id,
        &update,
        target_host,
    )
    .await?;
    Ok((StatusCode::OK, Json(resource)))
}
