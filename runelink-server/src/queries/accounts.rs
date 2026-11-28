use runelink_types::{Host, LocalAccount, UserRef};

use sqlx::PgExecutor;

use crate::{db::DbPool, error::ApiResult};

pub async fn insert(
    executor: impl PgExecutor<'_>,
    user_ref: UserRef,
    password_hash: &str,
) -> ApiResult<LocalAccount> {
    let local_account = sqlx::query_as!(
        LocalAccount,
        r#"
        INSERT INTO local_accounts (user_name, user_host, password_hash)
        VALUES ($1, $2, $3)
        RETURNING user_name, user_host AS "user_host: Host", password_hash,
                  created_at, updated_at;
        "#,
        user_ref.name,
        user_ref.host.as_str(),
        password_hash,
    )
    .fetch_one(executor)
    .await?;
    Ok(local_account)
}

pub async fn get_by_user(
    pool: &DbPool,
    user_ref: UserRef,
) -> ApiResult<LocalAccount> {
    let local_account = sqlx::query_as!(
        LocalAccount,
        r#"
        SELECT user_name, user_host AS "user_host: Host", password_hash,
               created_at, updated_at
        FROM local_accounts
        WHERE user_name = $1 AND user_host = $2;
        "#,
        user_ref.name,
        user_ref.host.as_str(),
    )
    .fetch_one(pool)
    .await?;
    Ok(local_account)
}

#[allow(dead_code)]
pub async fn delete_by_user(pool: &DbPool, user_ref: UserRef) -> ApiResult<()> {
    sqlx::query!(
        "DELETE FROM local_accounts WHERE user_name = $1 AND user_host = $2;",
        user_ref.name,
        user_ref.host.as_str(),
    )
    .execute(pool)
    .await?;
    Ok(())
}
