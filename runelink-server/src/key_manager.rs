use ed25519_dalek::{
    SigningKey, VerifyingKey,
    pkcs8::{
        DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey,
    },
};
use jsonwebtoken::{DecodingKey, EncodingKey};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use runelink_types::auth::PublicJwk;
use std::fs;
use std::path::PathBuf;

use crate::error::{ApiError, ApiResult};

/// Handles JWT signing keys and JWKS publication
#[allow(dead_code)]
#[derive(Clone)]
pub struct KeyManager {
    pub private_key: EncodingKey,
    pub decoding_key: DecodingKey,
    pub public_jwk: PublicJwk,
    pub path: PathBuf,
}

impl std::fmt::Debug for KeyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyManager")
            .field("private_key", &"[REDACTED]")
            .field("public_jwk", &self.public_jwk)
            .field("path", &self.path)
            .finish()
    }
}

impl KeyManager {
    /// Load keys if they exist under `path` or generate a new Ed25519 keypair
    pub fn load_or_generate(path: PathBuf) -> ApiResult<Self> {
        // Stored formats:
        // - private: PKCS#8 DER
        // - public:  SPKI (SubjectPublicKeyInfo) DER
        //
        // Note: jsonwebtoken's EdDSA verifier expects the *raw 32-byte*
        // Ed25519 public key, so we parse SPKI and convert to raw
        // for `DecodingKey`.
        let priv_path = path.join("private_ed25519.der");
        let pub_path = path.join("public_ed25519.der");

        if priv_path.exists() && pub_path.exists() {
            // Load from disk
            let priv_bytes = fs::read(&priv_path).map_err(|e| {
                ApiError::Internal(format!("failed to read private key: {e}"))
            })?;
            let pub_bytes = fs::read(&pub_path).map_err(|e| {
                ApiError::Internal(format!("failed to read public key: {e}"))
            })?;

            // Parse private key as PKCS#8 DER
            let signing_key =
                SigningKey::from_pkcs8_der(&priv_bytes).map_err(|e| {
                    ApiError::Internal(format!(
                        "invalid private key (expected PKCS#8 DER): {e}"
                    ))
                })?;

            // Parse public key as SPKI DER and convert to raw 32 bytes
            // for jsonwebtoken.
            let loaded_pub: [u8; 32] =
                VerifyingKey::from_public_key_der(&pub_bytes)
                    .map_err(|e| {
                        ApiError::Internal(format!(
                            "invalid public key (expected SPKI DER): {e}"
                        ))
                    })?
                    .to_bytes();

            // Ensure the public key matches the private key
            let derived_pub = signing_key.verifying_key().to_bytes();
            if derived_pub != loaded_pub {
                return Err(ApiError::Internal(
                    "public key does not match private key".into(),
                ));
            }

            let kid = "primary".to_string(); // TODO: should this change?
            Ok(Self {
                private_key: EncodingKey::from_ed_der(&priv_bytes),
                decoding_key: DecodingKey::from_ed_der(&loaded_pub),
                public_jwk: PublicJwk::from_ed25519_bytes(&loaded_pub, kid),
                path,
            })
        } else {
            // Generate new keypair
            let signing_key = SigningKey::generate(&mut UnwrapErr(SysRng));
            let verify_key = signing_key.verifying_key();
            let priv_pkcs8 = signing_key.to_pkcs8_der().map_err(|e| {
                ApiError::Internal(format!(
                    "failed to encode private key (pkcs8): {e}"
                ))
            })?;
            let pub_spki = verify_key.to_public_key_der().map_err(|e| {
                ApiError::Internal(format!(
                    "failed to encode public key (spki): {e}"
                ))
            })?;
            let pub_raw = verify_key.to_bytes();

            fs::create_dir_all(&path).map_err(|e| {
                ApiError::Internal(format!("failed to create keys dir: {e}"))
            })?;
            fs::write(&priv_path, priv_pkcs8.as_bytes()).map_err(|e| {
                ApiError::Internal(format!("failed to write private key: {e}"))
            })?;
            fs::write(&pub_path, pub_spki.as_bytes()).map_err(|e| {
                ApiError::Internal(format!("failed to write public key: {e}"))
            })?;
            println!("Generated new ed25519 keypair at {:?}", path);

            let kid = "primary".to_string(); // TODO: should this change?

            Ok(Self {
                private_key: EncodingKey::from_ed_der(priv_pkcs8.as_bytes()),
                decoding_key: DecodingKey::from_ed_der(&pub_raw),
                public_jwk: PublicJwk::from_ed25519_bytes(&pub_raw, kid),
                path,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use jsonwebtoken::{Algorithm, Header, Validation};
    use runelink_types::{ClientAccessClaims, UserRef};
    use time::Duration;
    use uuid::Uuid;

    #[test]
    fn loads_existing_der_keys_and_signs_jwts() {
        let path = std::env::temp_dir()
            .join(format!("runelink-existing-keys-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        // RFC 8032 test key, encoded as PKCS#8 and SPKI DER.
        fs::write(path.join("private_ed25519.der"), STANDARD.decode("MC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g").unwrap()).unwrap();
        fs::write(path.join("public_ed25519.der"), STANDARD.decode("MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=").unwrap()).unwrap();
        let keys = KeyManager::load_or_generate(path.clone()).unwrap();
        let claims = ClientAccessClaims::new(
            &UserRef::new("test".into(), "localhost".into()),
            "test".into(),
            "http://localhost:7000".into(),
            "openid".into(),
            Duration::minutes(5),
        );
        let token = jsonwebtoken::encode(
            &Header::new(Algorithm::EdDSA),
            &claims,
            &keys.private_key,
        )
        .unwrap();
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_audience(&["http://localhost:7000"]);
        validation.set_issuer(&["http://localhost:7000"]);
        let decoded = jsonwebtoken::decode::<ClientAccessClaims>(
            &token,
            &keys.decoding_key,
            &validation,
        )
        .unwrap();
        assert_eq!(decoded.claims, claims);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn generated_keys_survive_reload() {
        let path = std::env::temp_dir()
            .join(format!("runelink-generated-keys-{}", Uuid::new_v4()));
        let generated = KeyManager::load_or_generate(path.clone()).unwrap();
        let reloaded = KeyManager::load_or_generate(path.clone()).unwrap();
        assert_eq!(generated.public_jwk, reloaded.public_jwk);
        let claims = serde_json::json!({"exp": i64::MAX});
        let token = jsonwebtoken::encode(
            &Header::new(Algorithm::EdDSA),
            &claims,
            &generated.private_key,
        )
        .unwrap();
        assert!(
            jsonwebtoken::decode::<serde_json::Value>(
                &token,
                &reloaded.decoding_key,
                &Validation::new(Algorithm::EdDSA)
            )
            .is_ok()
        );
        fs::remove_dir_all(path).unwrap();
    }
}
