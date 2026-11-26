use runelink_types::{RefreshToken, UserRef};
use time::OffsetDateTime;

use crate::{db::DbPool, error::ApiResult};

pub async fn insert_refresh(
    pool: &DbPool,
    rt: &RefreshToken,
) -> ApiResult<RefreshToken> {
    let refresh_token = sqlx::query_as!(
        RefreshToken,
        r#"
        INSERT INTO refresh_tokens (token, user_name, user_host, client_id, issued_at,
                                    expires_at, revoked)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING token, user_name, user_host, client_id, issued_at, expires_at, revoked
        "#,
        rt.token,
        rt.user_name,
        rt.user_host,
        rt.client_id,
        rt.issued_at,
        rt.expires_at,
        rt.revoked,
    )
    .fetch_one(pool)
    .await?;
    Ok(refresh_token)
}

pub async fn get_refresh(
    pool: &DbPool,
    token_str: &str,
) -> ApiResult<RefreshToken> {
    let refresh_token = sqlx::query_as!(
        RefreshToken,
        r#"
        SELECT token, user_name, user_host, client_id, issued_at, expires_at, revoked
        FROM refresh_tokens
        WHERE token = $1
        "#,
        token_str,
    )
    .fetch_one(pool)
    .await?;
    Ok(refresh_token)
}

/// Makes a refresh token expire at `expires_at`, unless it expires sooner.
pub async fn expire_refresh_by(
    pool: &DbPool,
    token_str: &str,
    expires_at: OffsetDateTime,
) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET expires_at = LEAST(expires_at, $2)
        WHERE token = $1
        "#,
        token_str,
        expires_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes a user's refresh tokens that are revoked or expired by `now`.
pub async fn delete_stale_refresh(
    pool: &DbPool,
    user_ref: &UserRef,
    now: OffsetDateTime,
) -> ApiResult<()> {
    sqlx::query!(
        r#"
        DELETE FROM refresh_tokens
        WHERE user_name = $1 AND user_host = $2
            AND (revoked OR expires_at <= $3)
        "#,
        user_ref.name,
        user_ref.host,
        now,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn revoke_refresh(pool: &DbPool, token_str: &str) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET revoked = TRUE
        WHERE token = $1
        "#,
        token_str,
    )
    .execute(pool)
    .await?;
    Ok(())
}
