use runelink_types::{NewUser, User, UserRef, UserRole};
use sqlx::{Postgres, Transaction};
use time::{Duration, OffsetDateTime};

use crate::{
    db::DbPool,
    error::{ApiError, ApiResult},
};

/// Insert a new user, unless the name was deleted less than `reuse_delay` ago.
pub async fn insert(
    pool: &DbPool,
    new_user: &NewUser,
    reuse_delay: Duration,
) -> ApiResult<User> {
    let mut tx = pool.begin().await?;
    lock_user_name(&mut tx, &new_user.name, &new_user.host).await?;
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role)
        SELECT $1, $2, $3
        WHERE NOT EXISTS (
            SELECT 1 FROM deleted_users
            WHERE name = $1 AND host = $2
                AND deleted_at > NOW() - make_interval(secs => $4)
        )
        RETURNING
            name,
            host,
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        new_user.name,
        new_user.host,
        new_user.role as UserRole,
        reuse_delay.as_seconds_f64(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        ApiError::Conflict(
            "This name was recently deleted; try again later".into(),
        )
    })?;
    tx.commit().await?;
    Ok(user)
}

/// Serializes creating and deleting a user name until the transaction ends,
/// so a signup cannot miss a deletion's reservation that commits meanwhile.
async fn lock_user_name(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    host: &str,
) -> ApiResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{name}@{host}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn upsert_remote(
    pool: &DbPool,
    remote_user: &User,
) -> ApiResult<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role, created_at, updated_at, synced_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (name, host) DO UPDATE SET
            role = EXCLUDED.role,
            updated_at = EXCLUDED.updated_at,
            synced_at = EXCLUDED.synced_at
        RETURNING
            name,
            host,
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        remote_user.name,
        remote_user.host,
        UserRole::User as UserRole,
        remote_user.created_at,
        remote_user.updated_at,
        OffsetDateTime::now_utc(),
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_all(pool: &DbPool) -> ApiResult<Vec<User>> {
    let users = sqlx::query_as!(
        User,
        r#"
        SELECT
            name,
            host,
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at
        FROM users;
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

pub async fn ensure_exists(
    pool: &DbPool,
    user_ref: UserRef,
) -> ApiResult<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role)
        VALUES ($1, $2, 'user')
        ON CONFLICT (name, host) DO UPDATE SET updated_at = NOW()
        RETURNING
            name,
            host,
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        user_ref.name,
        user_ref.host,
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_by_ref(pool: &DbPool, user_ref: UserRef) -> ApiResult<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT
            name,
            host,
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at
        FROM users
        WHERE name = $1 AND host = $2;
        "#,
        user_ref.name,
        user_ref.host,
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

/// Delete a home user and record the deletion to block reusing the name for
/// `reuse_delay`. Records past that delay are removed along the way.
pub async fn delete_home(
    pool: &DbPool,
    user_ref: UserRef,
    reuse_delay: Duration,
) -> ApiResult<()> {
    // Outside the transaction, so concurrent deletions never lock the same
    // expired rows while holding different name locks.
    sqlx::query!(
        r#"
        DELETE FROM deleted_users
        WHERE deleted_at <= NOW() - make_interval(secs => $1);
        "#,
        reuse_delay.as_seconds_f64(),
    )
    .execute(pool)
    .await?;

    let mut tx = pool.begin().await?;
    lock_user_name(&mut tx, &user_ref.name, &user_ref.host).await?;
    let result = sqlx::query!(
        r#"
        WITH deleted AS (
            DELETE FROM users WHERE name = $1 AND host = $2
            RETURNING name, host
        )
        INSERT INTO deleted_users (name, host)
        SELECT name, host FROM deleted
        ON CONFLICT (name, host) DO UPDATE SET deleted_at = EXCLUDED.deleted_at;
        "#,
        user_ref.name,
        user_ref.host
    )
    .execute(&mut *tx)
    .await?;
    // A concurrent deletion already removed the user.
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete(pool: &DbPool, user_ref: UserRef) -> ApiResult<()> {
    sqlx::query!(
        "DELETE FROM users WHERE name = $1 AND host = $2;",
        user_ref.name,
        user_ref.host
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_associated_hosts(
    pool: &DbPool,
    user_ref: &UserRef,
) -> ApiResult<Vec<String>> {
    let hosts = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT s.host
        FROM user_remote_server_memberships m
        JOIN cached_remote_servers s ON s.id = m.remote_server_id
        WHERE m.user_name = $1 AND m.user_host = $2
        ORDER BY s.host ASC;
        "#,
        user_ref.name,
        user_ref.host,
    )
    .fetch_all(pool)
    .await?;
    Ok(hosts)
}
