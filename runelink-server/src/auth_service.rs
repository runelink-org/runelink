use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString, rand_core::OsRng},
};
use jsonwebtoken::{Algorithm, Header, Validation};
use runelink_types::{
    ClientAccessClaims, NewUser, RefreshToken, SignupRequest, TokenResponse,
    User, UserRef, UserRole,
    auth::{AuthTokenPasswordRequest, AuthTokenRefreshRequest},
};
use time::{Duration, OffsetDateTime};

use crate::{
    bearer_auth::ClientAuth,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

pub struct IssuedClientToken {
    pub user_ref: UserRef,
    pub response: TokenResponse,
}

pub fn authenticate_access_token(
    state: &AppState,
    access_token: &str,
) -> ApiResult<ClientAuth> {
    let server_id = state.config.api_url();
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(std::slice::from_ref(&server_id));
    validation.set_issuer(std::slice::from_ref(&server_id));

    let data = jsonwebtoken::decode::<ClientAccessClaims>(
        access_token,
        &state.key_manager.decoding_key,
        &validation,
    )
    .map_err(|_| ApiError::AuthError("Invalid or expired token".into()))?;

    Ok(ClientAuth {
        claims: data.claims,
    })
}

pub async fn signup(
    state: &AppState,
    request: SignupRequest,
) -> ApiResult<User> {
    let new_user = NewUser {
        name: request.name,
        host: state.config.local_host(),
        role: UserRole::User,
    };
    let user = queries::users::insert(&state.db_pool, &new_user).await?;

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(request.password.as_bytes(), &salt)
        .map_err(|error| ApiError::Internal(format!("hashing error: {error}")))?
        .to_string();

    let _ = queries::accounts::insert(
        &state.db_pool,
        user.as_ref(),
        &password_hash,
    )
    .await?;

    Ok(user)
}

pub async fn issue_password_token(
    state: &AppState,
    request: AuthTokenPasswordRequest,
) -> ApiResult<IssuedClientToken> {
    let client_id = request.client_id.unwrap_or_else(|| "default".into());
    let scope = request.scope.unwrap_or_else(|| "openid".into());

    let user = queries::users::get_by_ref(
        &state.db_pool,
        UserRef::new(request.username, state.config.local_host()),
    )
    .await?;

    let user_ref = user.as_ref();
    let account =
        queries::accounts::get_by_user(&state.db_pool, user_ref.clone())
            .await?;
    let parsed_hash = PasswordHash::new(&account.password_hash)
        .map_err(|_| ApiError::AuthError("invalid password hash".into()))?;
    Argon2::default()
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .map_err(|_| ApiError::AuthError("invalid credentials".into()))?;

    issue_client_token_response(state, user_ref, client_id, scope, None).await
}

pub async fn issue_refresh_token(
    state: &AppState,
    request: AuthTokenRefreshRequest,
) -> ApiResult<IssuedClientToken> {
    let refresh_token =
        queries::tokens::get_refresh(&state.db_pool, &request.refresh_token)
            .await?;

    let now = OffsetDateTime::now_utc();
    if refresh_token.revoked || refresh_token.expires_at <= now {
        return Err(ApiError::AuthError(
            "refresh token expired or revoked".into(),
        ));
    }

    let user_ref = UserRef::new(
        refresh_token.user_name.clone(),
        refresh_token.user_host.clone(),
    );
    let client_id =
        request.client_id.unwrap_or(refresh_token.client_id.clone());
    let scope = request.scope.unwrap_or_else(|| "openid".into());

    issue_client_token_response(
        state,
        user_ref,
        client_id,
        scope,
        Some(refresh_token.token),
    )
    .await
}

async fn issue_client_token_response(
    state: &AppState,
    user_ref: UserRef,
    client_id: String,
    scope: String,
    refresh_token: Option<String>,
) -> ApiResult<IssuedClientToken> {
    let lifetime = Duration::hours(1);
    let claims = ClientAccessClaims::new(
        &user_ref,
        client_id.clone(),
        state.config.api_url(),
        scope,
        lifetime,
    );
    let access_token = jsonwebtoken::encode(
        &Header::new(Algorithm::EdDSA),
        &claims,
        &state.key_manager.private_key,
    )
    .map_err(|error| ApiError::Internal(format!("jwt error: {error}")))?;

    let refresh_token = match refresh_token {
        Some(refresh_token) => refresh_token,
        None => {
            let token = RefreshToken::new(
                user_ref.clone(),
                client_id,
                Duration::days(30),
            );
            queries::tokens::insert_refresh(&state.db_pool, &token).await?;
            token.token
        }
    };

    Ok(IssuedClientToken {
        user_ref,
        response: TokenResponse {
            access_token,
            token_type: "Bearer".into(),
            expires_in: lifetime.whole_seconds(),
            refresh_token,
            scope: claims.scope,
        },
    })
}
