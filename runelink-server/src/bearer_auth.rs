use crate::{
    error::{ApiError, ApiResult},
    jwks_resolver,
    state::AppState,
};
use axum::http::HeaderMap;
use axum::http::header;
use jsonwebtoken::{Algorithm, Validation};
use runelink_types::{ClientAccessClaims, Host, UserRef};

fn extract_bearer_token(headers: &HeaderMap) -> ApiResult<String> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            ApiError::AuthError("Missing Authorization header".into())
        })?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            ApiError::AuthError("Invalid Authorization header format".into())
        })?
        .trim();
    Ok(token.into())
}

#[derive(Clone, Debug)]
pub struct ClientAuth {
    pub claims: ClientAccessClaims,
}

impl ClientAuth {
    pub fn from_headers(
        headers: &HeaderMap,
        state: &AppState,
    ) -> ApiResult<Self> {
        let token = extract_bearer_token(headers)?;
        let server_id = state.config.api_url();
        let mut validation = Validation::new(Algorithm::EdDSA);
        // Avoid using &[server_id.clone()] for performance
        validation.set_audience(std::slice::from_ref(&server_id));
        validation.set_issuer(std::slice::from_ref(&server_id));

        let data = jsonwebtoken::decode::<ClientAccessClaims>(
            &token,
            &state.key_manager.decoding_key,
            &validation,
        )
        .map_err(|_| ApiError::AuthError("Invalid or expired token".into()))?;

        Ok(Self {
            claims: data.claims,
        })
    }
}

/// A peer host, and the user it acts for, if any.
#[derive(Clone, Debug)]
pub struct FederationAuth {
    host: Host,
    user_ref: Option<UserRef>,
}

impl FederationAuth {
    /// A peer may only act for users whose home host is that peer.
    pub fn new(host: Host, user_ref: Option<UserRef>) -> ApiResult<Self> {
        if let Some(user_ref) = &user_ref
            && user_ref.host != host
        {
            return Err(ApiError::AuthError(format!(
                "Federation delegation mismatch: {host} cannot act for {user_ref}"
            )));
        }
        Ok(Self { host, user_ref })
    }

    pub async fn from_headers(
        headers: &HeaderMap,
        state: &AppState,
    ) -> ApiResult<Self> {
        let token = extract_bearer_token(headers)?;
        let expected_audience = state.config.api_url();
        let host = jwks_resolver::decode_federation_jwt(
            state,
            &token,
            &expected_audience,
        )
        .await?;
        // Tokens name only the peer, so it acts for no user yet.
        Ok(Self {
            host,
            user_ref: None,
        })
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn user_ref(&self) -> Option<&UserRef> {
        self.user_ref.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_headers(auth_value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = auth_value {
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
        }
        headers
    }

    #[test]
    fn test_valid_bearer_token() {
        let headers = make_headers(Some("Bearer abc.def.ghi"));
        let token = extract_bearer_token(&headers).expect("token should parse");
        assert_eq!(token, "abc.def.ghi");
    }

    #[test]
    fn test_missing_header() {
        let headers = make_headers(None);
        let err = extract_bearer_token(&headers).unwrap_err();
        match err {
            ApiError::AuthError(msg) => {
                assert!(msg.contains("Missing Authorization"))
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_wrong_prefix() {
        // Missing "Bearer " prefix
        let headers = make_headers(Some("Token abc.def.ghi"));
        let err = extract_bearer_token(&headers).unwrap_err();
        match err {
            ApiError::AuthError(msg) => {
                assert!(msg.contains("Invalid Authorization"))
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_empty_bearer_token() {
        let headers = make_headers(Some("Bearer "));
        let token =
            extract_bearer_token(&headers).expect("should parse but be empty");
        assert_eq!(token, "");
    }

    #[test]
    fn test_federation_delegation_requires_home_host() {
        let peer = Host::parse("peer.example").unwrap();
        let other = Host::parse("other.example").unwrap();
        let own_user = UserRef::new("alice".into(), peer.clone());
        let other_user = UserRef::new("bob".into(), other);
        assert!(FederationAuth::new(peer.clone(), None).is_ok());
        assert!(FederationAuth::new(peer.clone(), Some(own_user)).is_ok());
        assert!(FederationAuth::new(peer, Some(other_user)).is_err());
    }

    #[test]
    fn test_non_utf8_header() {
        use axum::http::HeaderValue;
        let mut headers = HeaderMap::new();
        // Invalid UTF-8 sequence
        let value = HeaderValue::from_bytes(b"Bearer \xFF\xFE").unwrap();
        headers.insert(header::AUTHORIZATION, value);
        let err = extract_bearer_token(&headers).unwrap_err();
        match err {
            ApiError::AuthError(msg) => {
                assert!(msg.contains("Missing Authorization"));
            }
            _ => panic!("unexpected error type"),
        }
    }
}
