use crate::{
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString, rand_core::OsRng},
};
use axum::{
    Form, Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use jsonwebtoken::{Algorithm, Header};
use log::info;
use reqwest::StatusCode;
use runelink_types::{
    ClientAccessClaims, JwksResponse, NewUser, OidcDiscoveryDocument,
    RefreshToken, SignupRequest, TokenRequest, TokenResponse, UserRef,
    UserRole,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

/// Creates a router for all auth-related endpoints
pub fn router() -> Router<AppState> {
    // Well-known discovery endpoints must be at root level
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/.well-known/jwks.json", get(jwks))
        // OAuth/OIDC endpoints under /auth
        .nest(
            "/auth",
            Router::new()
                .route("/token", post(token))
                .route("/userinfo", get(userinfo))
                .route("/register", post(register_client))
                .route("/signup", post(signup)),
        )
}

/// Discovery endpoint for OIDC
pub async fn discovery(
    State(state): State<AppState>,
) -> Json<OidcDiscoveryDocument> {
    info!("GET /.well-known/openid-configuration");
    let issuer = state.config.api_url();
    Json(OidcDiscoveryDocument {
        issuer: issuer.clone(),
        jwks_uri: format!("{issuer}/.well-known/jwks.json"),
        token_endpoint: format!("{issuer}/auth/token"),
        userinfo_endpoint: format!("{issuer}/auth/userinfo"),
        grant_types_supported: vec!["password".into(), "refresh_token".into()],
        response_types_supported: vec![],
        scopes_supported: vec![],
        token_endpoint_auth_methods_supported: vec!["none".into()],
    })
}

/// JWKS endpoint publishing public keys
pub async fn jwks(State(state): State<AppState>) -> Json<JwksResponse> {
    info!("GET /.well-known/jwks.json");
    Json(JwksResponse {
        keys: vec![state.key_manager.public_jwk.clone()],
    })
}

pub async fn token(
    State(state): State<AppState>,
    Form(req): Form<TokenRequest>,
) -> ApiResult<impl IntoResponse> {
    info!("POST /auth/token?grant_type={}", req.grant_type);
    // TODO: check dynamic client IDs for validity
    let client_id = req.client_id.unwrap_or_else(|| "default".into());
    // TODO: check requested scopes for validity
    let scope = req.scope.unwrap_or_else(|| "openid".into());

    match req.grant_type.as_str() {
        "password" => {
            let username = req
                .username
                .clone()
                .ok_or(ApiError::BadRequest("missing username".into()))?;
            let password = req
                .password
                .clone()
                .ok_or(ApiError::BadRequest("missing password".into()))?;

            // Get user
            let user = queries::users::get_by_ref(
                &state.db_pool,
                UserRef::new(username, state.config.local_host()),
            )
            .await?;

            // Verify password hash
            let user_ref = user.as_ref();
            let account = queries::accounts::get_by_user(
                &state.db_pool,
                user_ref.clone(),
            )
            .await?;
            let parsed_hash = PasswordHash::new(&account.password_hash)
                .map_err(|_| {
                    ApiError::AuthError("invalid password hash".into())
                })?;
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .map_err(|_| {
                    ApiError::AuthError("invalid credentials".into())
                })?;

            // Create client access JWT (valid only on this server)
            let lifetime = Duration::hours(1);
            let claims = ClientAccessClaims::new(
                &user_ref,
                client_id.clone(),
                state.config.api_url(),
                scope,
                lifetime,
            );
            let token = jsonwebtoken::encode(
                &Header::new(Algorithm::EdDSA),
                &claims,
                &state.key_manager.private_key,
            )
            .map_err(|e| ApiError::Internal(format!("jwt error: {e}")))?;

            // Create refresh token
            let rt = RefreshToken::new(user_ref, client_id, Duration::days(30));
            queries::tokens::insert_refresh(&state.db_pool, &rt).await?;

            Ok((
                StatusCode::OK,
                Json(TokenResponse {
                    access_token: token,
                    token_type: "Bearer".into(),
                    expires_in: 3600,
                    refresh_token: rt.token,
                    scope: claims.scope,
                }),
            ))
        }

        "refresh_token" => {
            let rtoken = req
                .refresh_token
                .clone()
                .ok_or(ApiError::BadRequest("missing refresh_token".into()))?;
            let rt =
                queries::tokens::get_refresh(&state.db_pool, &rtoken).await?;

            // Validate refresh token
            let now = OffsetDateTime::now_utc();
            if rt.revoked || rt.expires_at <= now {
                return Err(ApiError::AuthError(
                    "refresh token expired or revoked".into(),
                ));
            }

            // Create new client access JWT
            let user_ref =
                UserRef::new(rt.user_name.clone(), rt.user_host.clone());
            let lifetime = Duration::hours(1);
            let claims = ClientAccessClaims::new(
                &user_ref,
                client_id,
                state.config.api_url(),
                scope,
                lifetime,
            );
            let token = jsonwebtoken::encode(
                &Header::new(Algorithm::EdDSA),
                &claims,
                &state.key_manager.private_key,
            )
            .map_err(|e| ApiError::Internal(format!("jwt error: {e}")))?;

            Ok((
                StatusCode::OK,
                Json(TokenResponse {
                    access_token: token,
                    token_type: "Bearer".into(),
                    expires_in: lifetime.whole_seconds(),
                    refresh_token: rt.token, // TODO: token rotation
                    scope: claims.scope,
                }),
            ))
        }

        _ => Err(ApiError::BadRequest("unsupported grant_type".into())),
    }
}

/// Protected endpoint returning user claims (stubbed for now)
pub async fn userinfo() -> Json<serde_json::Value> {
    info!("GET /auth/userinfo");
    // TODO: Implement userinfo endpoint with actual user data
    Json(json!({
        "error": "not_implemented",
        "message": "Userinfo endpoint not yet implemented"
    }))
}

/// Dynamic Client Registration endpoint (stubbed for now)
pub async fn register_client() -> Json<serde_json::Value> {
    info!("POST /auth/register");
    // TODO: Implement client registration, generating client_id
    Json(json!({
        "error": "not_implemented",
        "message": "Client registration not yet implemented"
    }))
}

/// POST /auth/signup
pub async fn signup(
    State(state): State<AppState>,
    Json(req): Json<SignupRequest>,
) -> ApiResult<impl IntoResponse> {
    info!("POST /auth/signup\nsignup_request = {:#?}", req);
    // Insert user
    let new_user = NewUser {
        name: req.name,
        host: state.config.local_host(),
        role: UserRole::User,
    };
    let user = queries::users::insert(&state.db_pool, &new_user).await?;

    // Hash password
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(format!("hashing error: {e}")))?
        .to_string();

    // Insert local account
    let _ = queries::accounts::insert(
        &state.db_pool,
        user.as_ref(),
        &password_hash,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(user)))
}
