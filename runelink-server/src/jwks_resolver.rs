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

/// Kid -> raw public key bytes (currently ed25519 raw 32 bytes).
type KeysByKid = HashMap<String, Vec<u8>>;

#[derive(Debug, Clone)]
pub struct CachedJwks {
    fetched_at: OffsetDateTime,
    /// The issuer's keys, or `None` if fetching them failed.
    keys_by_kid: Option<KeysByKid>,
}

impl CachedJwks {
    fn is_fresh(&self, now: OffsetDateTime) -> bool {
        // Failures are cached briefly, so that repeated tokens from an
        // unreachable issuer don't each fetch its keys again.
        let ttl = match self.keys_by_kid {
            Some(_) => Duration::minutes(10),
            None => Duration::minutes(1),
        };
        self.fetched_at + ttl > now
    }
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

/// Fetches an issuer's keys, or the reason it failed, which is only logged,
/// since the issuer is unverified and its response must not reach whoever
/// sent the token.
async fn fetch_jwks(
    state: &AppState,
    issuer: &Host,
) -> Result<KeysByKid, String> {
    // The HTTP client's resolver checks host names, but not literal IPs.
    if !state.peer_policy.allows_literal(issuer) {
        return Err("federation may not connect to its address".into());
    }
    let url = format!(
        "{}/.well-known/jwks.json",
        issuer.api_url(state.config.secure)
    );
    let response = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(status.to_string());
    }
    let jwks: JwksResponse =
        response.json().await.map_err(|e| e.to_string())?;

    let mut keys_by_kid = HashMap::new();
    for key in jwks.keys {
        // For now we only support Ed25519 OKP keys as produced by KeyManager.
        // Future: validate kty/crv/alg/use_ and support rotation.
        let pub_bytes =
            URL_SAFE_NO_PAD.decode(key.x).map_err(|e| e.to_string())?;
        keys_by_kid.insert(key.kid, pub_bytes);
    }
    Ok(keys_by_kid)
}

async fn get_cached_jwks(
    state: &AppState,
    issuer: &Host,
) -> ApiResult<KeysByKid> {
    let now = OffsetDateTime::now_utc();
    let cached = state
        .jwks_cache
        .read()
        .await
        .get(issuer)
        .filter(|entry| entry.is_fresh(now))
        .cloned();
    let entry = match cached {
        Some(entry) => entry,
        None => {
            let keys_by_kid = fetch_jwks(state, issuer)
                .await
                .inspect_err(|reason| {
                    log::warn!("Failed fetching JWKS from {issuer}: {reason}")
                })
                .ok();
            let entry = CachedJwks {
                fetched_at: now,
                keys_by_kid,
            };
            let mut cache = state.jwks_cache.write().await;
            // Issuers are unverified, so stale entries are dropped rather
            // than kept for every issuer ever named.
            cache.retain(|_, entry| entry.is_fresh(now));
            cache.insert(issuer.clone(), entry.clone());
            entry
        }
    };
    entry.keys_by_kid.ok_or_else(|| {
        ApiError::AuthError("could not fetch the issuer's signing keys".into())
    })
}

fn select_public_key_bytes<'a>(
    keys_by_kid: &'a KeysByKid,
    kid: Option<&str>,
) -> ApiResult<&'a [u8]> {
    if let Some(kid) = kid {
        return keys_by_kid
            .get(kid)
            .map(|v| v.as_slice())
            .ok_or_else(|| ApiError::AuthError("unknown jwk kid".into()));
    }

    if keys_by_kid.len() == 1 {
        return Ok(keys_by_kid.values().next().unwrap().as_slice());
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

    let keys_by_kid = get_cached_jwks(state, &issuer).await?;
    let pub_bytes =
        select_public_key_bytes(&keys_by_kid, header.kid.as_deref())?;

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
