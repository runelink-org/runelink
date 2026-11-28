#![allow(dead_code)]

use axum::http::HeaderMap;
use runelink_types::{
    auth::FederationClaims,
    server::{ServerMembership, ServerRef, ServerRole},
    user::{User, UserRef},
};

use crate::{
    bearer_auth::{ClientAuth, FederationAuth},
    config::ServerConfig,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Macro to construct an `And` requirement.
/// Example: `and!(Requirement::Client, Requirement::User(user_ref))`
#[macro_export]
macro_rules! and {
    () => {
        $crate::auth::Requirement::And(vec![])
    };
    ( $( $req:expr ),+ $(,)? ) => {
        $crate::auth::Requirement::And(vec![ $( $req ),+ ])
    };
}

/// Macro to construct an `Or` requirement.
/// Example: `or!(Requirement::Client, Requirement::Federation)`
#[macro_export]
macro_rules! or {
    () => {
        $crate::auth::Requirement::Or(vec![])
    };
    ( $( $req:expr ),+ $(,)? ) => {
        $crate::auth::Requirement::Or(vec![ $( $req ),+ ])
    };
}

#[derive(Clone, Debug)]
pub enum Principal {
    Client(ClientAuth),
    Federation(FederationAuth),
}

impl Principal {
    pub fn from_client_headers(
        headers: &HeaderMap,
        state: &AppState,
    ) -> ApiResult<Self> {
        let auth = ClientAuth::from_headers(headers, state)?;
        Ok(Self::Client(auth))
    }

    pub async fn from_federation_headers(
        headers: &HeaderMap,
        state: &AppState,
    ) -> ApiResult<Self> {
        let auth = FederationAuth::from_headers(headers, state).await?;
        Ok(Self::Federation(auth))
    }
}

#[derive(Clone, Debug)]
pub enum Requirement {
    /// Must be authenticated with a client token.
    Client,
    /// Must be authenticated with a federation token.
    Federation,
    /// Must be a delegated federated user with the referenced identity.
    FederatedUser(UserRef),
    /// Must be a user with the referenced identity.
    User(UserRef),
    /// Must be a host admin.
    HostAdmin,
    /// Must be a member of the referenced server.
    ServerMember(ServerRef),
    /// Must be an admin of the referenced server.
    ServerAdmin(ServerRef),
    /// A requirement that will always be satisfied.
    Always,
    /// A requirement that will never be satisfied.
    Never,
    /// Must satisfy all sub-requirements.
    And(Vec<Requirement>),
    /// Must satisfy at least one sub-requirement.
    Or(Vec<Requirement>),
}

impl Requirement {
    pub fn or_admin(self) -> Self {
        // Checked last, since it is rarely the requirement that holds.
        or!(self, Requirement::HostAdmin)
    }

    pub fn client_only(self) -> Self {
        and!(Requirement::Client, self)
    }

    pub fn federated_only(self) -> Self {
        and!(Requirement::Federation, self)
    }

    /// Check the requirement, returning why it fails, if it does.
    async fn check(
        &self,
        auth: &Authorization,
        config: &ServerConfig,
        facts: &mut impl Facts,
    ) -> ApiResult<Option<String>> {
        match self {
            Requirement::Client => {
                if !matches!(auth.principal, Some(Principal::Client(_))) {
                    return Ok(Some("Client auth required".into()));
                }
            }

            Requirement::Federation => {
                if !matches!(auth.principal, Some(Principal::Federation(_))) {
                    return Ok(Some("Federation auth required".into()));
                }
            }

            Requirement::FederatedUser(expected) => {
                let Some(Principal::Federation(federation)) = &auth.principal
                else {
                    return Ok(Some("Federation auth required".into()));
                };
                let Some(user_ref) = federation.claims.user_ref.as_ref() else {
                    return Ok(Some(
                        "Federated delegated user required".into(),
                    ));
                };
                let expected_iss = expected.host.api_url(config.secure);
                if federation.claims.iss != expected_iss {
                    return Ok(Some(
                        "Federation issuer does not match delegated user host"
                            .into(),
                    ));
                }
                if user_ref != expected {
                    return Ok(Some("Invalid delegated federated user".into()));
                }
            }

            Requirement::User(expected) => {
                let user = auth.user(facts).await?;
                if !user.is_some_and(|user| user.as_ref() == *expected) {
                    return Ok(Some("Invalid user".into()));
                }
            }

            Requirement::HostAdmin => {
                let user = auth.user(facts).await?;
                if !user.is_some_and(|user| user.is_admin()) {
                    return Ok(Some("Host admin only".into()));
                }
            }

            Requirement::ServerMember(server) => {
                let role = auth.server_role(facts, server).await?;
                if role.is_none() {
                    return Ok(Some("Server member only".into()));
                }
            }

            Requirement::ServerAdmin(server) => {
                let role = auth.server_role(facts, server).await?;
                if role != Some(ServerRole::Admin) {
                    return Ok(Some("Server admin only".into()));
                }
            }

            Requirement::Always => {
                return Ok(None);
            }

            Requirement::Never => {
                return Ok(Some("Requirement can not be satisfied".into()));
            }

            Requirement::And(reqs) => {
                for req in reqs {
                    if let Some(error) =
                        Box::pin(req.check(auth, config, facts)).await?
                    {
                        return Ok(Some(error));
                    }
                }
            }

            Requirement::Or(reqs) => {
                if reqs.is_empty() {
                    return Ok(Some("No requirements".into()));
                }
                let mut errors = Vec::<String>::new();
                for req in reqs {
                    match Box::pin(req.check(auth, config, facts)).await? {
                        Some(error) => errors.push(error),
                        None => return Ok(None),
                    }
                }
                let combined_error = match errors.as_slice() {
                    [error] => error.clone(),
                    errors => errors
                        .iter()
                        .map(|e| format!("({e})"))
                        .collect::<Vec<String>>()
                        .join(" and "),
                };
                return Ok(Some(combined_error));
            }
        }
        Ok(None)
    }
}

/// The state that requirements are checked against. Requests are checked once
/// against the database before they are queued, and mutations again inside
/// their actor against its current state, so that a permission revoked
/// meanwhile is always respected.
pub trait Facts: Send {
    /// The user, if they exist on this host.
    fn user(
        &mut self,
        user_ref: &UserRef,
    ) -> impl Future<Output = ApiResult<Option<User>>> + Send;

    /// The user's role in a server, if they are a member.
    fn server_role(
        &mut self,
        server_ref: &ServerRef,
        user_ref: &UserRef,
    ) -> impl Future<Output = ApiResult<Option<ServerRole>>> + Send;
}

/// Who a request acts for and what it was authorized with, so that the
/// requirement can be checked again where the request runs.
#[derive(Clone, Debug)]
pub struct Authorization {
    principal: Option<Principal>,
    user_ref: Option<UserRef>,
    requirement: Requirement,
}

impl Authorization {
    /// For changes made by the host itself rather than for a request.
    pub fn host() -> Self {
        Self {
            principal: None,
            user_ref: None,
            requirement: Requirement::Always,
        }
    }

    /// A user's request with no principal, for tests of what actors check.
    #[cfg(test)]
    pub fn for_test_user(user_ref: UserRef, requirement: Requirement) -> Self {
        Self {
            principal: None,
            user_ref: Some(user_ref),
            requirement,
        }
    }

    /// The requesting user, if they exist on this host.
    async fn user(&self, facts: &mut impl Facts) -> ApiResult<Option<User>> {
        match &self.user_ref {
            Some(user_ref) => facts.user(user_ref).await,
            None => Ok(None),
        }
    }

    /// The requesting user's role in a server, if they are a member.
    async fn server_role(
        &self,
        facts: &mut impl Facts,
        server_ref: &ServerRef,
    ) -> ApiResult<Option<ServerRole>> {
        match &self.user_ref {
            Some(user_ref) => facts.server_role(server_ref, user_ref).await,
            None => Ok(None),
        }
    }

    /// Check the requirement, failing with an auth error if it does not hold.
    pub async fn check(
        &self,
        config: &ServerConfig,
        facts: &mut impl Facts,
    ) -> ApiResult<()> {
        match self.requirement.check(self, config, facts).await? {
            Some(error) => Err(ApiError::AuthError(error)),
            None => Ok(()),
        }
    }
}

/// Facts read from the database, cached for one request. Only its request's
/// own user is ever looked up, so one cached user is enough.
struct DbFacts<'a> {
    state: &'a AppState,
    user: Option<Option<User>>,
    memberships: Option<Vec<ServerMembership>>,
}

impl Facts for DbFacts<'_> {
    async fn user(&mut self, user_ref: &UserRef) -> ApiResult<Option<User>> {
        if self.user.is_none() {
            let user = queries::users::find_by_ref(
                &*self.state.db_pool,
                user_ref.clone(),
            )
            .await?;
            self.user = Some(user);
        }
        Ok(self.user.clone().flatten())
    }

    async fn server_role(
        &mut self,
        server_ref: &ServerRef,
        user_ref: &UserRef,
    ) -> ApiResult<Option<ServerRole>> {
        if self.memberships.is_none() {
            let memberships =
                queries::memberships::get_by_user(self.state, user_ref.clone())
                    .await?;
            self.memberships = Some(memberships);
        }
        Ok(self.memberships.as_ref().and_then(|memberships| {
            memberships
                .iter()
                .find(|membership| {
                    membership.server.id == server_ref.id
                        && membership.server.host == server_ref.host
                })
                .map(|membership| membership.role)
        }))
    }
}

/// Session represents the authenticated context for a request.
///
/// For client auth, the user is always local and exists in the DB.
/// For federation auth, the user reference may or may not exist locally.
#[derive(Clone, Debug)]
pub struct Session {
    /// The authenticated principal (Client or Federation)
    pub principal: Principal,
    /// Optional delegated user reference (always present for client auth, optional for federation)
    pub user_ref: Option<UserRef>,
    /// Present only when the request was authenticated via federation
    pub federation: Option<FederationClaims>,
    /// Cached user lookup result (None = not looked up, Some(None) = looked up but not found, Some(Some(user)) = found)
    cached_user: Option<Option<User>>,
    /// What the request was authorized with
    authorization: Authorization,
}

impl Session {
    /// What this request was authorized with, for actors to check again.
    pub fn authorization(&self) -> Authorization {
        self.authorization.clone()
    }

    /// Perform a lazy DB lookup of the delegated user (cached).
    /// Returns Ok(None) if the user does not exist locally.
    pub async fn lookup_user(
        &mut self,
        state: &AppState,
    ) -> ApiResult<Option<User>> {
        // If already cached, return the cached result
        if let Some(cached) = &self.cached_user {
            return Ok(cached.clone());
        }
        // No user delegated
        let Some(user_ref) = &self.user_ref else {
            self.cached_user = Some(None);
            return Ok(None);
        };
        let user =
            queries::users::find_by_ref(&*state.db_pool, user_ref.clone())
                .await?;
        self.cached_user = Some(user.clone());
        Ok(user)
    }

    /// Require that a delegated user exists locally.
    /// Returns an error if the user reference is missing or the user is not in the DB.
    pub async fn require_user(&mut self, state: &AppState) -> ApiResult<User> {
        let user_ref = self.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("No delegated user in session".into())
        })?;
        let user = self.lookup_user(state).await?.ok_or_else(|| {
            ApiError::AuthError(format!("User {user_ref} not found locally"))
        })?;
        Ok(user)
    }
}

/// Authorization engine (shared).
pub async fn authorize(
    state: &AppState,
    principal: Principal,
    req: Requirement,
) -> ApiResult<Session> {
    // Extract user identity from the principal (no DB lookups yet)
    let (user_ref, federation_claims) = match &principal {
        Principal::Client(auth) => {
            let user_ref = UserRef::parse_subject(&auth.claims.sub)
                .ok_or_else(|| {
                    ApiError::AuthError(
                        "Invalid token subject (expected name@host)".into(),
                    )
                })?;
            (Some(user_ref), None)
        }
        Principal::Federation(auth) => {
            (auth.claims.user_ref.clone(), Some(auth.claims.clone()))
        }
    };
    let authorization = Authorization {
        principal: Some(principal.clone()),
        user_ref: user_ref.clone(),
        requirement: req,
    };
    let mut facts = DbFacts {
        state,
        user: None,
        memberships: None,
    };
    authorization.check(&state.config, &mut facts).await?;
    let cached_user = if user_ref.is_some() {
        facts.user
    } else {
        // Not delegated
        Some(None)
    };
    let session = Session {
        principal,
        user_ref,
        federation: federation_claims,
        cached_user,
        authorization,
    };
    Ok(session)
}
