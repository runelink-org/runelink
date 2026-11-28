use std::time::Duration;

use runelink_types::{
    host::Host,
    ws::{FederationWsReply, FederationWsRequest},
};

use crate::{
    auth::Session,
    error::{ApiError, ApiResult},
    state::AppState,
};

// Leave room for the peer's database acquisition timeout and query execution.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Send a request to a peer host and take the value out of the reply it
/// expects, on behalf of the session's user if there is a session:
///
/// ```ignore
/// federation::request!(
///     state,
///     host,
///     Some(session),
///     FederationWsRequest::ServersDelete { server_id },
///     FederationWsReply::ServersDelete => ()
/// )
/// ```
macro_rules! request {
    (
        $state:expr,
        $host:expr,
        $session:expr,
        $request:expr,
        $reply:pat => $value:expr $(,)?
    ) => {{
        let host = $host;
        match $crate::ops::federation::send($state, host, $session, $request)
            .await
        {
            Ok($reply) => Ok($value),
            Ok(_) => Err($crate::error::ApiError::Internal(format!(
                "Unexpected federation reply from {host}, expected {}",
                stringify!($reply)
            ))),
            Err(error) => Err(error),
        }
    }};
}
pub(super) use request;

/// Send a request to a peer host, which may only reply with its own
/// resources. Use [`request!`] to take the expected reply.
pub(super) async fn send(
    state: &AppState,
    host: &Host,
    session: Option<&Session>,
    request: FederationWsRequest,
) -> ApiResult<FederationWsReply> {
    let delegated_user_ref = session
        .map(|session| {
            session.user_ref.clone().ok_or_else(|| {
                ApiError::Internal(
                    "User reference required for federated requests".into(),
                )
            })
        })
        .transpose()?;
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
