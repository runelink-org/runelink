use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use log::debug;
use runelink_types::{
    channel::TextChannelRef,
    host::Host,
    message::{MessageRef, NewMessage},
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
pub struct MessageQueryParams {
    pub target_host: Option<Host>,
}

/// POST /text_channels/{host}/{id}/messages
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(text_channel_ref): Path<TextChannelRef>,
    Json(new_message): Json<NewMessage>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "POST /text_channels/{}/{}/messages\nnew_message = {:#?}",
        text_channel_ref.host, text_channel_ref.id, new_message
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::create(
            &state,
            &text_channel_ref,
            &new_message.author,
        )
        .await?,
    )
    .await?;
    let message = ops::messages::create(
        &state,
        &session,
        &text_channel_ref,
        &new_message,
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
    debug!("GET /messages?target_host={:?}", params.target_host);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_all(),
    )
    .await?;
    let messages =
        ops::messages::get_all(&state, &session, params.target_host.as_ref())
            .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /servers/{host}/{id}/messages
pub async fn get_by_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_ref): Path<ServerRef>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "GET /servers/{}/{}/messages",
        server_ref.host, server_ref.id
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_server(&server_ref),
    )
    .await?;
    let messages =
        ops::messages::get_by_server(&state, &session, &server_ref).await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /text_channels/{host}/{id}/messages
pub async fn get_by_text_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(text_channel_ref): Path<TextChannelRef>,
) -> ApiResult<impl IntoResponse> {
    debug!(
        "GET /text_channels/{}/{}/messages",
        text_channel_ref.host, text_channel_ref.id
    );
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_text_channel(&state, &text_channel_ref)
            .await?,
    )
    .await?;
    let messages =
        ops::messages::get_by_text_channel(&state, &session, &text_channel_ref)
            .await?;
    Ok((StatusCode::OK, Json(messages)))
}

/// GET /messages/{host}/{id}
pub async fn get_by_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_ref): Path<MessageRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("GET /messages/{}/{}", message_ref.host, message_ref.id);
    let session = authorize(
        &state,
        Principal::from_client_headers(&headers, &state)?,
        ops::messages::auth::get_by_id(&state, &message_ref).await?,
    )
    .await?;
    let message =
        ops::messages::get_by_id(&state, &session, &message_ref).await?;
    Ok((StatusCode::OK, Json(message)))
}

/// DELETE /messages/{host}/{id}
pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_ref): Path<MessageRef>,
) -> ApiResult<impl IntoResponse> {
    debug!("DELETE /messages/{}/{}", message_ref.host, message_ref.id);
    let principal = Principal::from_client_headers(&headers, &state)?;
    let target = ops::messages::locate(&state, &message_ref).await?;
    let session =
        authorize(&state, principal, ops::messages::auth::delete(&target))
            .await?;
    let message = ops::messages::delete(&state, &session, target).await?;
    Ok((StatusCode::OK, Json(message)))
}

/// PATCH /messages/{host}/{id}
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_ref): Path<MessageRef>,
    Json(update): Json<runelink_types::message::MessageUpdate>,
) -> ApiResult<impl IntoResponse> {
    let principal = Principal::from_client_headers(&headers, &state)?;
    let target = ops::messages::locate(&state, &message_ref).await?;
    let session =
        authorize(&state, principal, ops::messages::auth::update(&target))
            .await?;
    let resource =
        ops::messages::update(&state, &session, &update, target).await?;
    Ok((StatusCode::OK, Json(resource)))
}
