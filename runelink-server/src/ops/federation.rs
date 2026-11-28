use std::time::Duration;

use runelink_types::{
    host::Host,
    user::UserRef,
    ws::{FederationWsReply, FederationWsRequest},
};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

// Leave room for the peer's database acquisition timeout and query execution.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Send a request to a peer host, which may only reply with its own
/// resources.
pub(super) async fn request(
    state: &AppState,
    host: &Host,
    delegated_user_ref: Option<UserRef>,
    request: FederationWsRequest,
) -> ApiResult<FederationWsReply> {
    let request_future =
        Box::pin(state.federation_ws_manager.send_request_to_host(
            state,
            host,
            delegated_user_ref,
            request,
            REQUEST_TIMEOUT,
        ));
    let reply = request_future
        .await
        .map_err(|error| error.into_api_error(host))?;
    if !reply.is_owned_by(host) {
        return Err(ApiError::Internal(format!(
            "{host} replied with resources of another host"
        )));
    }
    Ok(reply)
}
