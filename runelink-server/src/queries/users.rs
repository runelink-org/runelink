use runelink_types::{NewUser, User, UserRef, UserRole};
use time::OffsetDateTime;

use crate::{db::DbPool, error::ApiResult};

pub async fn insert(pool: &DbPool, new_user: &NewUser) -> ApiResult<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role)
        VALUES ($1, $2, $3)
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
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
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
