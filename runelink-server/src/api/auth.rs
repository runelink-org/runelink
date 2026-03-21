use axum::{
    Form, Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use log::info;
use reqwest::StatusCode;
use runelink_types::{
    JwksResponse, OidcDiscoveryDocument, SignupRequest, TokenRequest,
    auth::{AuthTokenPasswordRequest, AuthTokenRefreshRequest},
};
use serde_json::json;

use crate::{
    auth_service,
    error::{ApiError, ApiResult},
    state::AppState,
};

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
            let issued = auth_service::issue_password_token(
                &state,
                AuthTokenPasswordRequest {
                    username: req.username.ok_or(ApiError::BadRequest(
                        "missing username".into(),
                    ))?,
                    password: req.password.ok_or(ApiError::BadRequest(
                        "missing password".into(),
                    ))?,
                    scope: Some(scope),
                    client_id: Some(client_id),
                },
            )
            .await?;

            Ok((StatusCode::OK, Json(issued.response)))
        }

        "refresh_token" => {
            let issued = auth_service::issue_refresh_token(
                &state,
                AuthTokenRefreshRequest {
                    refresh_token: req.refresh_token.ok_or(
                        ApiError::BadRequest("missing refresh_token".into()),
                    )?,
                    scope: Some(scope),
                    client_id: Some(client_id),
                },
            )
            .await?;

            Ok((StatusCode::OK, Json(issued.response)))
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
    let user = auth_service::signup(&state, req).await?;

    Ok((StatusCode::CREATED, Json(user)))
}
