use runelink_types::{Host, NewUser, User, UserRef, UserRole};
use sqlx::{PgConnection, PgExecutor};
use time::{Duration, OffsetDateTime};

use crate::{
    db::DbPool,
    error::{ApiError, ApiResult},
};

/// Insert a new user, unless the name was deleted less than `reuse_delay` ago.
pub async fn insert(
    conn: &mut PgConnection,
    new_user: &NewUser,
    reuse_delay: Duration,
) -> ApiResult<User> {
    lock_unless_recently_deleted(
        conn,
        &new_user.name,
        &new_user.host,
        reuse_delay,
    )
    .await?;
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role)
        VALUES ($1, $2, $3)
        RETURNING
            name,
            host AS "host: Host",
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        new_user.name,
        new_user.host.as_str(),
        new_user.role as UserRole,
    )
    .fetch_one(conn)
    .await?;
    Ok(user)
}

/// Take the name lock (see [`lock_user_name`]), failing if the name was deleted
/// less than `reuse_delay` ago.
async fn lock_unless_recently_deleted(
    conn: &mut PgConnection,
    name: &str,
    host: &Host,
    reuse_delay: Duration,
) -> ApiResult<()> {
    lock_user_name(conn, name, host).await?;
    let recently_deleted = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM deleted_users
            WHERE name = $1 AND host = $2
                AND deleted_at > NOW() - make_interval(secs => $3)
        ) AS "exists!";
        "#,
        name,
        host.as_str(),
        reuse_delay.as_seconds_f64(),
    )
    .fetch_one(conn)
    .await?;
    if recently_deleted {
        return Err(ApiError::Conflict(
            "This name was recently deleted; try again later".into(),
        ));
    }
    Ok(())
}

/// Serializes creating and deleting a user name until the transaction ends,
/// so a signup cannot miss a deletion's reservation that commits meanwhile.
async fn lock_user_name(
    conn: &mut PgConnection,
    name: &str,
    host: &Host,
) -> ApiResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{name}@{host}"))
        .execute(conn)
        .await?;
    Ok(())
}

/// Store a remote user's record, unless it was deleted less than
/// `reuse_delay` ago, so that a request still in flight cannot recreate it.
pub async fn upsert_remote(
    conn: &mut PgConnection,
    remote_user: &User,
    reuse_delay: Duration,
) -> ApiResult<User> {
    lock_unless_recently_deleted(
        conn,
        &remote_user.name,
        &remote_user.host,
        reuse_delay,
    )
    .await?;
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
            host AS "host: Host",
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        remote_user.name,
        remote_user.host.as_str(),
        UserRole::User as UserRole,
        remote_user.created_at,
        remote_user.updated_at,
        OffsetDateTime::now_utc(),
    )
    .fetch_one(conn)
    .await?;
    Ok(user)
}

pub async fn get_all(pool: &DbPool) -> ApiResult<Vec<User>> {
    let users = sqlx::query_as!(
        User,
        r#"
        SELECT
            name,
            host AS "host: Host",
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

/// Store a record for a remote user, like [`upsert_remote`].
pub async fn ensure_exists(
    conn: &mut PgConnection,
    user_ref: UserRef,
    reuse_delay: Duration,
) -> ApiResult<User> {
    lock_unless_recently_deleted(
        conn,
        &user_ref.name,
        &user_ref.host,
        reuse_delay,
    )
    .await?;
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (name, host, role)
        VALUES ($1, $2, 'user')
        ON CONFLICT (name, host) DO UPDATE SET updated_at = NOW()
        RETURNING
            name,
            host AS "host: Host",
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at;
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_one(conn)
    .await?;
    Ok(user)
}

/// Like [`get_by_ref`], with `None` for an unknown user.
pub async fn find_by_ref(
    executor: impl PgExecutor<'_>,
    user_ref: UserRef,
) -> ApiResult<Option<User>> {
    match get_by_ref(executor, user_ref).await {
        Ok(user) => Ok(Some(user)),
        Err(ApiError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn get_by_ref(
    executor: impl PgExecutor<'_>,
    user_ref: UserRef,
) -> ApiResult<User> {
    let user = sqlx::query_as!(
        User,
        r#"
        SELECT
            name,
            host AS "host: Host",
            role AS "role: UserRole",
            created_at,
            updated_at,
            synced_at
        FROM users
        WHERE name = $1 AND host = $2;
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_one(executor)
    .await?;
    Ok(user)
}

/// Delete a user and record the deletion, which blocks reusing a home user's
/// name, or recreating a remote user's record, for `reuse_delay`. Records past
/// that delay are removed along the way. Fails with `NotFound` if the user
/// does not exist.
pub async fn delete(
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
    let deleted = sqlx::query!(
        "DELETE FROM users WHERE name = $1 AND host = $2",
        user_ref.name,
        user_ref.host.as_str()
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(ApiError::NotFound);
    }
    insert_deleted(&mut tx, &user_ref).await?;
    tx.commit().await?;
    Ok(())
}

/// Record a user as deleted without deleting their record yet, so that
/// [`upsert_remote`] and [`ensure_exists`] refuse it from now on.
pub async fn mark_deleted(pool: &DbPool, user_ref: &UserRef) -> ApiResult<()> {
    let mut tx = pool.begin().await?;
    lock_user_name(&mut tx, &user_ref.name, &user_ref.host).await?;
    insert_deleted(&mut tx, user_ref).await?;
    tx.commit().await?;
    Ok(())
}

async fn insert_deleted(
    conn: &mut PgConnection,
    user_ref: &UserRef,
) -> ApiResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO deleted_users (name, host) VALUES ($1, $2)
        ON CONFLICT (name, host) DO UPDATE SET deleted_at = EXCLUDED.deleted_at;
        "#,
        user_ref.name,
        user_ref.host.as_str()
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn get_associated_hosts(
    pool: &DbPool,
    user_ref: &UserRef,
) -> ApiResult<Vec<Host>> {
    let hosts = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT remote_server_host AS "remote_server_host: Host"
        FROM user_remote_server_memberships
        WHERE user_name = $1 AND user_host = $2
        ORDER BY remote_server_host ASC;
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_all(pool)
    .await?;
    Ok(hosts)
}
