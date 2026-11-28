#![allow(dead_code)]

use runelink_types::{
    host::Host,
    server::{
        NewServerMembership, Server, ServerId, ServerMember, ServerMembership,
        ServerRef, ServerRole,
    },
    user::{User, UserRef},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use time::OffsetDateTime;

use crate::{
    config::ServerConfig,
    db::DbPool,
    error::{ApiError, ApiResult},
    state::AppState,
};

/// An intermediate type needed because of sqlx limitations
#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
struct ServerMemberRow {
    pub user: Option<Json<User>>,
    pub role: ServerRole,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl TryFrom<ServerMemberRow> for ServerMember {
    type Error = ApiError;

    fn try_from(row: ServerMemberRow) -> Result<Self, Self::Error> {
        let user = row.user.ok_or(ApiError::Unknown("User is null".into()))?.0;
        Ok(ServerMember {
            user,
            role: row.role,
            joined_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(sqlx::FromRow, Debug)]
struct ServerMembershipRow {
    server_id: Option<ServerId>,
    server_title: Option<String>,
    server_description: Option<String>,
    server_host_from_db: Option<Host>,
    server_created_at: Option<OffsetDateTime>,
    server_updated_at: Option<OffsetDateTime>,
    user_name: Option<String>,
    user_host: Option<Host>,
    role: Option<ServerRole>,
    created_at: Option<OffsetDateTime>,
    updated_at: Option<OffsetDateTime>,
    synced_at: Option<OffsetDateTime>,
}

impl ServerMembershipRow {
    fn try_into_server_membership(
        self,
        config: &ServerConfig,
    ) -> ApiResult<ServerMembership> {
        let server_host = self
            .server_host_from_db
            .unwrap_or_else(|| config.public_host().clone());

        // Needed because of weird sqlx limitations (or misuse)
        let get_error = || ApiError::Unknown("Sqlx conversion error".into());
        Ok(ServerMembership {
            server: Server {
                id: self.server_id.ok_or_else(get_error)?,
                title: self.server_title.ok_or_else(get_error)?,
                description: self.server_description,
                host: server_host,
                created_at: self.server_created_at.ok_or_else(get_error)?,
                updated_at: self.server_updated_at.ok_or_else(get_error)?,
            },
            user_ref: UserRef::new(
                self.user_name.ok_or_else(get_error)?,
                self.user_host.ok_or_else(get_error)?,
            ),
            role: self.role.ok_or_else(get_error)?,
            joined_at: self.created_at.ok_or_else(get_error)?,
            updated_at: self.updated_at.ok_or_else(get_error)?,
            synced_at: self.synced_at,
        })
    }
}

pub async fn upsert_local(
    conn: &mut PgConnection,
    new_membership: &NewServerMembership,
) -> ApiResult<ServerMember> {
    sqlx::query!(
        r#"
        INSERT INTO server_users (server_id, user_name, user_host, role)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (server_id, user_name, user_host) DO UPDATE
            SET role = EXCLUDED.role,
                updated_at = NOW();
        "#,
        new_membership.server_ref.id.as_uuid(),
        new_membership.user_ref.name,
        new_membership.user_ref.host.as_str(),
        new_membership.role as ServerRole,
    )
    .execute(&mut *conn)
    .await?;
    get_local_member_by_user_and_server(
        conn,
        new_membership.server_ref.id,
        new_membership.user_ref.clone(),
    )
    .await
}

/// Cache a local user's membership of a remote server. Returns whether it was
/// not cached before.
pub async fn insert_remote(
    executor: impl PgExecutor<'_>,
    membership: &ServerMembership,
) -> ApiResult<bool> {
    // Rows that a conflict updates have a nonzero `xmax`.
    let inserted = sqlx::query_scalar!(
        r#"
        INSERT INTO user_remote_server_memberships (
            user_name, user_host, remote_server_host, remote_server_id, role,
            remote_created_at, remote_updated_at, synced_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        ON CONFLICT (
            user_name, user_host, remote_server_host, remote_server_id
        ) DO UPDATE
            SET role = EXCLUDED.role,
                remote_created_at = EXCLUDED.remote_created_at,
                remote_updated_at = EXCLUDED.remote_updated_at,
                synced_at = NOW()
        RETURNING (xmax = 0) AS "inserted!"
        "#,
        membership.user_ref.name,
        membership.user_ref.host.as_str(),
        membership.server.host.as_str(),
        membership.server.id.as_uuid(),
        membership.role as ServerRole,
        membership.joined_at,
        membership.updated_at,
    )
    .fetch_one(executor)
    .await?;
    Ok(inserted)
}

pub async fn get_local_member_by_user_and_server(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
    user_ref: UserRef,
) -> ApiResult<ServerMember> {
    sqlx::query_as!(
        ServerMemberRow,
        r#"
        SELECT
            to_jsonb(u) "user: Json<User>",
            su.role AS "role: ServerRole",
            su.created_at,
            su.updated_at
        FROM users u
        JOIN server_users su ON u.name = su.user_name AND u.host = su.user_host
        WHERE su.server_id = $1 AND su.user_name = $2 AND su.user_host = $3
        ORDER BY u.name, u.host
        "#,
        server_id.as_uuid(),
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_one(executor)
    .await?
    .try_into()
}

pub async fn get_members_by_server(
    pool: &DbPool,
    server_id: ServerId,
) -> ApiResult<Vec<ServerMember>> {
    sqlx::query_as!(
        ServerMemberRow,
        r#"
        SELECT
            to_jsonb(u) "user: Json<User>",
            su.role AS "role: ServerRole",
            su.created_at,
            su.updated_at
        FROM users u
        JOIN server_users su ON u.name = su.user_name AND u.host = su.user_host
        WHERE su.server_id = $1
        ORDER BY u.name, u.host
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ServerMember::try_from)
    .collect()
}

/// The members of a local server and their roles.
pub async fn get_local_roles(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
) -> ApiResult<Vec<(UserRef, ServerRole)>> {
    let rows = sqlx::query!(
        r#"
        SELECT user_name, user_host AS "user_host: Host", role AS "role: ServerRole"
        FROM server_users
        WHERE server_id = $1;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (UserRef::new(row.user_name, row.user_host), row.role))
        .collect())
}

/// The local servers a user belongs to.
pub async fn get_local_server_ids_for_user(
    executor: impl PgExecutor<'_>,
    user_ref: &UserRef,
) -> ApiResult<Vec<ServerId>> {
    let ids = sqlx::query_scalar!(
        "SELECT server_id FROM server_users WHERE user_name = $1 AND user_host = $2",
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_all(executor)
    .await?;
    Ok(ids.into_iter().map(Into::into).collect())
}

/// The cached local members of a cached remote server, or `None` for servers
/// that are not cached.
pub async fn get_remote_server_members(
    pool: &DbPool,
    server_ref: &ServerRef,
) -> ApiResult<Option<Vec<UserRef>>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            m.user_name AS "user_name?",
            m.user_host AS "user_host?: Host"
        FROM cached_remote_servers s
        LEFT JOIN user_remote_server_memberships m
            ON m.remote_server_host = s.host AND m.remote_server_id = s.id
        WHERE s.host = $1 AND s.id = $2
        "#,
        server_ref.host.as_str(),
        server_ref.id.as_uuid(),
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let users = rows
        .into_iter()
        .filter_map(|row| Some(UserRef::new(row.user_name?, row.user_host?)))
        .collect();
    Ok(Some(users))
}

pub async fn get_local_by_user_and_server(
    executor: impl PgExecutor<'_>,
    config: &ServerConfig,
    server_id: ServerId,
    user_ref: UserRef,
) -> ApiResult<ServerMembership> {
    let row = sqlx::query!(
        r#"
        SELECT
            s.id,
            s.title,
            s.description,
            s.created_at AS server_created_at,
            s.updated_at AS server_updated_at,
            su.role AS "role: ServerRole",
            su.created_at AS membership_created_at,
            su.updated_at AS membership_updated_at
        FROM servers s
        JOIN server_users su
            ON s.id = su.server_id
        WHERE s.id = $1
            AND su.user_name = $2 AND su.user_host = $3
        "#,
        server_id.as_uuid(),
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_one(executor)
    .await?;

    Ok(ServerMembership {
        server: Server {
            id: row.id.into(),
            host: config.public_host().clone(),
            title: row.title,
            description: row.description,
            created_at: row.server_created_at,
            updated_at: row.server_updated_at,
        },
        user_ref,
        role: row.role,
        joined_at: row.membership_created_at,
        updated_at: row.membership_updated_at,
        synced_at: None,
    })
}

pub async fn get_by_user(
    state: &AppState,
    user_ref: UserRef,
) -> ApiResult<Vec<ServerMembership>> {
    let rows = sqlx::query_as!(
        ServerMembershipRow,
        r#"
        -- Local server memberships
        SELECT
            s.id AS "server_id: ServerId",
            s.title AS server_title,
            s.description AS server_description,
            NULL::TEXT AS "server_host_from_db: Host",
            s.created_at AS server_created_at,
            s.updated_at AS server_updated_at,
            su.user_name AS user_name,
            su.user_host AS "user_host: Host",
            su.role AS "role!: Option<ServerRole>",
            su.created_at,
            su.updated_at,
            NULL::TIMESTAMPTZ AS synced_at
        FROM servers s
        JOIN server_users su ON s.id = su.server_id
        WHERE su.user_name = $1 AND su.user_host = $2

        UNION ALL

        -- Cached remote server memberships
        SELECT
            crs.id AS "server_id: ServerId",
            crs.title AS server_title,
            crs.description AS server_description,
            crs.host AS server_host_from_db,
            crs.remote_created_at AS server_created_at,
            crs.remote_updated_at AS server_updated_at,
            ursm.user_name AS user_name,
            ursm.user_host AS user_host,
            ursm.role AS "role!: Option<ServerRole>",
            ursm.remote_created_at AS created_at,
            ursm.remote_updated_at AS updated_at,
            ursm.synced_at AS synced_at
        FROM cached_remote_servers crs
        JOIN user_remote_server_memberships ursm
            ON crs.host = ursm.remote_server_host
                AND crs.id = ursm.remote_server_id
        WHERE ursm.user_name = $1 AND ursm.user_host = $2

        ORDER BY server_title ASC
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_all(state.db_pool.as_ref())
    .await?;

    rows.into_iter()
        .map(|row| row.try_into_server_membership(&state.config))
        .collect()
}

/// Delete a local server membership.
pub async fn delete_local(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
    user_ref: UserRef,
) -> ApiResult<()> {
    let result = sqlx::query!(
        r#"
        DELETE FROM server_users
        WHERE server_id = $1 AND user_name = $2 AND user_host = $3
        "#,
        server_id.as_uuid(),
        user_ref.name,
        user_ref.host.as_str(),
    )
    .execute(executor)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

pub async fn delete_remote(
    pool: &DbPool,
    server_ref: &ServerRef,
    user_ref: UserRef,
) -> ApiResult<()> {
    let result = sqlx::query!(
        r#"
        DELETE FROM user_remote_server_memberships
        WHERE remote_server_host = $1 AND remote_server_id = $2
            AND user_name = $3 AND user_host = $4
        "#,
        server_ref.host.as_str(),
        server_ref.id.as_uuid(),
        user_ref.name,
        user_ref.host.as_str(),
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// The other members of the local servers a user is in, or of only the given
/// ones.
pub async fn get_local_co_members(
    executor: impl PgExecutor<'_>,
    user_ref: &UserRef,
    server_ids: Option<&[ServerId]>,
) -> ApiResult<Vec<UserRef>> {
    let server_ids = server_ids
        .map(|ids| ids.iter().map(ServerId::as_uuid).collect::<Vec<_>>());
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT
            other.user_name,
            other.user_host AS "user_host: Host"
        FROM server_users own
        JOIN server_users other ON other.server_id = own.server_id
        WHERE own.user_name = $1 AND own.user_host = $2
            AND ($3::UUID[] IS NULL OR own.server_id = ANY($3))
            AND NOT (other.user_name = $1 AND other.user_host = $2)
        "#,
        user_ref.name,
        user_ref.host.as_str(),
        server_ids.as_deref(),
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| UserRef::new(row.user_name, row.user_host))
        .collect())
}

/// The hosts of the cached remote servers a local user is in.
pub async fn get_remote_server_hosts_for_user(
    executor: impl PgExecutor<'_>,
    user_ref: &UserRef,
) -> ApiResult<Vec<Host>> {
    let hosts = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT remote_server_host AS "host: Host"
        FROM user_remote_server_memberships
        WHERE user_name = $1 AND user_host = $2
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_all(executor)
    .await?;
    Ok(hosts)
}

/// The local users who are in a cached remote server on `host`.
pub async fn get_local_users_in_servers_on(
    executor: impl PgExecutor<'_>,
    host: &Host,
) -> ApiResult<Vec<UserRef>> {
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT user_name, user_host AS "user_host: Host"
        FROM user_remote_server_memberships
        WHERE remote_server_host = $1
        "#,
        host.as_str(),
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| UserRef::new(row.user_name, row.user_host))
        .collect())
}

/// Each local user or member of a local server, and each member from `host`
/// who shares a local server with them.
pub async fn get_local_co_members_from(
    executor: impl PgExecutor<'_>,
    host: &Host,
) -> ApiResult<Vec<(UserRef, UserRef)>> {
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT
            member.user_name,
            member.user_host AS "user_host: Host",
            peer.user_name AS peer_name,
            peer.user_host AS "peer_host: Host"
        FROM server_users peer
        JOIN server_users member ON member.server_id = peer.server_id
        WHERE peer.user_host = $1
            AND NOT (
                member.user_name = peer.user_name
                AND member.user_host = peer.user_host
            )
        "#,
        host.as_str(),
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                UserRef::new(row.user_name, row.user_host),
                UserRef::new(row.peer_name, row.peer_host),
            )
        })
        .collect())
}
