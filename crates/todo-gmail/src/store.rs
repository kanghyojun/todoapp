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

pub(crate) async fn set_history_id(
    pool: &SqlitePool,
    account_id: &str,
    history_id: &str,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE gmail_accounts SET history_id = ?, last_synced_at = ?, sync_state = 'idle', \
         last_error = NULL WHERE id = ?",
    )
    .bind(history_id)
    .bind(now_string())
    .bind(account_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_needs_auth(pool: &SqlitePool, email: &str) -> Result<(), Error> {
    sqlx::query("UPDATE gmail_accounts SET sync_state = 'needs_auth' WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) struct MessageMeta {
    pub gmail_id: String,
    pub thread_id: String,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub snippet: String,
    pub internal_date: i64,
    pub in_inbox: bool,
    pub is_unread: bool,
}

/// 메타데이터만 upsert 한다. 본문 컬럼(body_*)은 건드리지 않는다.
pub(crate) async fn upsert_message_meta(
    pool: &SqlitePool,
    account_id: &str,
    meta: &MessageMeta,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO gmail_messages \
         (account_id, gmail_id, thread_id, from_name, from_email, subject, snippet, \
          internal_date, in_inbox, is_unread, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(account_id, gmail_id) DO UPDATE SET \
           thread_id = excluded.thread_id, from_name = excluded.from_name, \
           from_email = excluded.from_email, subject = excluded.subject, \
           snippet = excluded.snippet, internal_date = excluded.internal_date, \
           in_inbox = excluded.in_inbox, is_unread = excluded.is_unread, \
           updated_at = excluded.updated_at",
    )
    .bind(account_id)
    .bind(&meta.gmail_id)
    .bind(&meta.thread_id)
    .bind(&meta.from_name)
    .bind(&meta.from_email)
    .bind(&meta.subject)
    .bind(&meta.snippet)
    .bind(meta.internal_date)
    .bind(meta.in_inbox)
    .bind(meta.is_unread)
    .bind(now_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// history 의 labelsAdded/labelsRemoved 를 로컬에 반영한다. INBOX/UNREAD 만 관심 대상.
pub(crate) async fn apply_label_change(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
    labels: &[String],
    added: bool,
) -> Result<(), Error> {
    let value = i64::from(added);
    let now = now_string();
    if labels.iter().any(|label| label == "INBOX") {
        sqlx::query(
            "UPDATE gmail_messages SET in_inbox = ?, updated_at = ? \
             WHERE account_id = ? AND gmail_id = ?",
        )
        .bind(value)
        .bind(&now)
        .bind(account_id)
        .bind(gmail_id)
        .execute(pool)
        .await?;
    }
    if labels.iter().any(|label| label == "UNREAD") {
        sqlx::query(
            "UPDATE gmail_messages SET is_unread = ?, updated_at = ? \
             WHERE account_id = ? AND gmail_id = ?",
        )
        .bind(value)
        .bind(&now)
        .bind(account_id)
        .bind(gmail_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn delete_message(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
) -> Result<(), Error> {
    sqlx::query("DELETE FROM gmail_messages WHERE account_id = ? AND gmail_id = ?")
        .bind(account_id)
        .bind(gmail_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, Error> {
    Ok(
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

pub(crate) async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}
