use std::time::Duration;

use runelink_types::{
    user::UserRef,
    ws::{FederationWsReply, FederationWsRequest},
};

use crate::{error::ApiResult, state::AppState};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn request(
    state: &AppState,
    host: &str,
    delegated_user_ref: Option<UserRef>,
    request: FederationWsRequest,
) -> ApiResult<FederationWsReply> {
    let request_future = Box::pin(state.federation_ws_manager.send_request_to_host(
        state,
        host,
        delegated_user_ref,
        request,
        REQUEST_TIMEOUT,
    ));
    request_future
        .await
        .map_err(|error| error.into_api_error(host))
}
