use chrono::{SecondsFormat, Utc};
use sqlx::SqlitePool;

use crate::error::Error;
use crate::model::GmailAccount;

const COLORS: [&str; 6] = [
    "#268bd2", "#2aa198", "#859900", "#b58900", "#d33682", "#cb4b16",
];

pub(crate) fn color_for_index(index: i64) -> String {
    COLORS[(index as usize) % COLORS.len()].to_owned()
}

pub(crate) fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

const ACCOUNT_COLUMNS: &str =
    "id, email, color, history_id, sync_state, last_error, last_synced_at, added_at";

pub async fn insert_account(pool: &SqlitePool, email: &str) -> Result<GmailAccount, Error> {
    if let Some(existing) = fetch_account_by_email(pool, email).await? {
        return Ok(existing);
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gmail_accounts")
        .fetch_one(pool)
        .await?;
    let id = uuid::Uuid::new_v4().to_string();
    let color = color_for_index(count);
    let now = now_string();
    sqlx::query(
        "INSERT INTO gmail_accounts (id, email, color, sync_state, added_at) \
         VALUES (?, ?, ?, 'idle', ?)",
    )
    .bind(&id)
    .bind(email)
    .bind(&color)
    .bind(&now)
    .execute(pool)
    .await?;
    fetch_account_by_email(pool, email)
        .await?
        .ok_or(Error::AccountNotFound)
}

async fn fetch_account_by_email(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<GmailAccount>, Error> {
    Ok(sqlx::query_as::<_, GmailAccount>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM gmail_accounts WHERE email = ?"
    ))
    .bind(email)
    .fetch_optional(pool)
    .await?)
}

pub async fn fetch_account(pool: &SqlitePool, id: &str) -> Result<GmailAccount, Error> {
    sqlx::query_as::<_, GmailAccount>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM gmail_accounts WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::AccountNotFound)
}

pub async fn list_accounts(pool: &SqlitePool) -> Result<Vec<GmailAccount>, Error> {
    Ok(sqlx::query_as::<_, GmailAccount>(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM gmail_accounts ORDER BY added_at ASC, email ASC"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn delete_account(pool: &SqlitePool, id: &str) -> Result<(), Error> {
    sqlx::query("DELETE FROM gmail_accounts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
