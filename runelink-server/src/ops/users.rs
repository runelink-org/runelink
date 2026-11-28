use runelink_types::{
    host::Host,
    user::{NewUser, User, UserRef},
    ws::{FederationWsReply, FederationWsRequest},
};

use super::federation;
use crate::{
    auth::Session,
    config::Locality,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new user on this host, without an account. Signup creates users
/// with accounts.
pub async fn create(
    state: &AppState,
    session: &Session,
    new_user: &NewUser,
) -> ApiResult<User> {
    if state.config.locality(&new_user.host).is_remote() {
        return Err(ApiError::BadRequest(
            "Users can only be created for this host".into(),
        ));
    }
    // Stored under this host's own name, however the request spelled it.
    let new_user = NewUser {
        host: state.config.public_host().clone(),
        ..new_user.clone()
    };
    state
        .actors
        .create_user(new_user, None, session.authorization())
        .await
}

/// List all users (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&Host>,
) -> ApiResult<Vec<User>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let users = queries::users::get_all(&state.db_pool).await?;
            Ok(users)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            None,
            FederationWsRequest::UsersGetAll,
            FederationWsReply::UsersGetAll(users) => users
        ),
    }
}

/// Find a user by UserRef (public).
pub async fn get_by_ref(
    state: &AppState,
    user_ref: UserRef,
) -> ApiResult<User> {
    if state.config.locality(&user_ref.host).is_local() {
        let user =
            queries::users::get_by_ref(&*state.db_pool, user_ref).await?;
        Ok(user)
    } else {
        let host = user_ref.host.clone();
        federation::request!(
            state,
            &host,
            None,
            FederationWsRequest::UsersGetByRef { user_ref },
            FederationWsReply::UsersGetByRef(user) => user
        )
    }
}

/// Delete a user from their home server.
pub async fn delete_home_user(
    state: &AppState,
    session: &Session,
    user_ref: &UserRef,
) -> ApiResult<()> {
    if state.config.locality(&user_ref.host).is_remote() {
        return Err(ApiError::BadRequest(
            "Can only delete users from their home server".into(),
        ));
    }
    state
        .actors
        .delete_home_user(user_ref.clone(), session.authorization())
        .await
}

/// Delete the record of the remote user a peer acts for, which only their
/// home host can do (see `FederationAuth::new`).
pub async fn delete_remote_user_record(
    state: &AppState,
    session: &Session,
) -> ApiResult<()> {
    let user_ref = session.user_ref.as_ref().ok_or_else(|| {
        ApiError::AuthError("A delegated user is required".into())
    })?;
    if state.config.locality(&user_ref.host).is_local() {
        return Err(ApiError::BadRequest(
            "Cannot delete local users via federation".into(),
        ));
    }

    state
        .actors
        .delete_remote_user(user_ref.clone(), session.authorization())
        .await
}

/// Get all hosts associated with a user (public).
pub async fn get_associated_hosts(
    state: &AppState,
    user_ref: UserRef,
    target_host: Option<&Host>,
) -> ApiResult<Vec<Host>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let hosts =
                queries::users::get_associated_hosts(&state.db_pool, &user_ref)
                    .await?;
            Ok(hosts)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            None,
            FederationWsRequest::UsersGetAssociatedHosts { user_ref },
            FederationWsReply::UsersGetAssociatedHosts(hosts) => hosts
        ),
    }
}

/// Auth requirements for user operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn delete(user_ref: UserRef) -> Req {
        Req::User(user_ref).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub fn delete(user_ref: UserRef) -> Req {
            Req::FederatedUser(user_ref).federated_only()
        }
    }
}
