#![allow(dead_code)]

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use runelink_types::{FederationClaims, Host, PublicJwk};
use serde::Deserialize;
use time::{Duration, OffsetDateTime};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct CachedJwks {
    fetched_at: OffsetDateTime,
    // kid -> raw public key bytes (currently ed25519 raw 32 bytes)
    keys_by_kid: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<PublicJwk>,
}

#[derive(Deserialize)]
struct IssOnly {
    iss: String,
}

fn parse_iss_unverified(token: &str) -> ApiResult<String> {
    // We need `iss` to locate the JWKS before we can verify the signature.
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(ApiError::AuthError("invalid JWT format".into()));
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).map_err(|e| {
        ApiError::AuthError(format!("invalid JWT payload: {e}"))
    })?;
    let parsed: IssOnly = serde_json::from_slice(&payload).map_err(|e| {
        ApiError::AuthError(format!("invalid JWT payload json: {e}"))
    })?;
    Ok(parsed.iss)
}

async fn fetch_jwks(state: &AppState, issuer: &Host) -> ApiResult<CachedJwks> {
    let url = format!(
        "{}/.well-known/jwks.json",
        issuer.api_url(state.config.secure)
    );
    // Only logged, since the issuer is unverified and its response must not
    // reach whoever sent the token.
    let failed = |reason: String| {
        log::warn!("Failed fetching JWKS from {issuer}: {reason}");
        ApiError::AuthError("could not fetch the issuer's signing keys".into())
    };
    let response = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|e| failed(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(failed(status.to_string()));
    }
    let jwks: JwksResponse =
        response.json().await.map_err(|e| failed(e.to_string()))?;

    let mut keys_by_kid = HashMap::new();
    for key in jwks.keys {
        // For now we only support Ed25519 OKP keys as produced by KeyManager.
        // Future: validate kty/crv/alg/use_ and support rotation.
        let pub_bytes = URL_SAFE_NO_PAD
            .decode(key.x)
            .map_err(|e| failed(e.to_string()))?;
        keys_by_kid.insert(key.kid, pub_bytes);
    }

    Ok(CachedJwks {
        fetched_at: OffsetDateTime::now_utc(),
        keys_by_kid,
    })
}

async fn get_cached_jwks(
    state: &AppState,
    issuer: &Host,
) -> ApiResult<CachedJwks> {
    // Simple time-based cache. This will be expanded later with better
    // negative caching and rotation support.
    let ttl = Duration::minutes(10);
    {
        let cache = state.jwks_cache.read().await;
        if let Some(entry) = cache.get(issuer)
            && entry.fetched_at + ttl > OffsetDateTime::now_utc()
        {
            return Ok(entry.clone());
        }
    }

    let fetched = fetch_jwks(state, issuer).await?;
    {
        let mut cache = state.jwks_cache.write().await;
        cache.insert(issuer.clone(), fetched.clone());
    }
    Ok(fetched)
}

fn select_public_key_bytes<'a>(
    cached: &'a CachedJwks,
    kid: Option<&str>,
) -> ApiResult<&'a [u8]> {
    if let Some(kid) = kid {
        return cached
            .keys_by_kid
            .get(kid)
            .map(|v| v.as_slice())
            .ok_or_else(|| ApiError::AuthError("unknown jwk kid".into()));
    }

    if cached.keys_by_kid.len() == 1 {
        return Ok(cached.keys_by_kid.values().next().unwrap().as_slice());
    }

    Err(ApiError::AuthError(
        "missing kid and multiple jwks keys available".into(),
    ))
}

/// Validate a federation JWT (server-to-server delegated authority) by
/// discovering the issuer's JWKS via `{iss}/.well-known/jwks.json`.
///
/// Notes:
/// - This does an **unverified** parse of `iss` from the JWT payload solely to
///   locate the JWKS. Signature and claim validation happens after the key is
///   fetched.
/// - Audience enforcement is performed via `expected_audience`.
pub async fn decode_federation_jwt(
    state: &AppState,
    token: &str,
    expected_audience: &str,
) -> ApiResult<(Host, FederationClaims)> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| ApiError::AuthError(format!("invalid JWT header: {e}")))?;
    let iss = parse_iss_unverified(token)?;
    // Keys are only fetched from a host's exact API URL.
    let issuer =
        Host::from_api_url(&iss, state.config.secure).ok_or_else(|| {
            ApiError::AuthError(
                "federation issuer is not a host's API URL".into(),
            )
        })?;

    let cached = get_cached_jwks(state, &issuer).await?;
    let pub_bytes = select_public_key_bytes(&cached, header.kid.as_deref())?;

    if pub_bytes.len() != 32 {
        return Err(ApiError::AuthError(
            "invalid jwks ed25519 key length".into(),
        ));
    }
    let decoding_key = DecodingKey::from_ed_der(pub_bytes);

    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(&[expected_audience]);
    validation.set_issuer(&[iss.as_str()]);

    let data = jsonwebtoken::decode::<FederationClaims>(
        token,
        &decoding_key,
        &validation,
    )
    .map_err(|_| ApiError::AuthError("invalid or expired token".into()))?;
    let claims = data.claims;

    // Verify delegation policy: issuer can only delegate users from their own host
    if let Some(user_ref) = &claims.user_ref
        && user_ref.host != issuer
    {
        return Err(ApiError::AuthError(format!(
            "Federation delegation mismatch: token from {issuer} cannot delegate user from {}",
            user_ref.host
        )));
    }

    Ok((issuer, claims))
}
