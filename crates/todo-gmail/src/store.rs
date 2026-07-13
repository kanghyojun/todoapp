use chrono::{SecondsFormat, Utc};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};

use crate::error::Error;
use crate::model::{GmailAccount, MailBody, MailFilter, MailFolder, MailListItem};

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

/// 본문이 이미 페치돼 있으면 반환한다. 미페치(body_fetched_at NULL)면 None.
pub(crate) async fn read_body(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
) -> Result<Option<MailBody>, Error> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT body_text, body_html, body_fetched_at FROM gmail_messages \
         WHERE account_id = ? AND gmail_id = ?",
    )
    .bind(account_id)
    .bind(gmail_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(text, html, fetched)| {
        fetched.map(|_| MailBody {
            gmail_id: gmail_id.to_owned(),
            body_text: text,
            body_html: html,
        })
    }))
}

pub(crate) async fn write_body(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
    text: Option<&str>,
    html: Option<&str>,
) -> Result<(), Error> {
    let now = now_string();
    sqlx::query(
        "UPDATE gmail_messages SET body_text = ?, body_html = ?, body_fetched_at = ?, \
         updated_at = ? WHERE account_id = ? AND gmail_id = ?",
    )
    .bind(text)
    .bind(html)
    .bind(&now)
    .bind(&now)
    .bind(account_id)
    .bind(gmail_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn query_messages(
    pool: &SqlitePool,
    filter: &MailFilter,
) -> Result<Vec<MailListItem>, Error> {
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT m.account_id AS account_id, a.email AS account_email, a.color AS account_color, \
         m.gmail_id AS gmail_id, m.thread_id AS thread_id, m.from_name AS from_name, \
         m.from_email AS from_email, m.subject AS subject, m.snippet AS snippet, \
         m.internal_date AS internal_date, m.in_inbox AS in_inbox, m.is_unread AS is_unread \
         FROM gmail_messages m JOIN gmail_accounts a ON a.id = m.account_id WHERE 1 = 1",
    );
    match filter.folder {
        MailFolder::Inbox => {
            builder.push(" AND m.in_inbox = 1");
        }
        MailFolder::Archive => {
            builder.push(" AND m.in_inbox = 0");
        }
        MailFolder::All => {}
    }
    if let Some(account_id) = &filter.account_id {
        builder.push(" AND m.account_id = ").push_bind(account_id.clone());
    }
    if let Some(query) = filter.query.as_deref() {
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        builder
            .push(" AND (m.subject LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\' OR m.from_name LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\' OR m.from_email LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\' OR m.snippet LIKE ")
            .push_bind(like)
            .push(" ESCAPE '\\')");
    }
    builder.push(" ORDER BY m.internal_date DESC");
    let limit = filter.limit.unwrap_or(200).min(1_000);
    builder.push(" LIMIT ").push_bind(i64::from(limit));
    Ok(builder
        .build_query_as::<MailListItem>()
        .fetch_all(pool)
        .await?)
}

pub(crate) async fn local_set_inbox(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
    in_inbox: bool,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE gmail_messages SET in_inbox = ?, updated_at = ? \
         WHERE account_id = ? AND gmail_id = ?",
    )
    .bind(i64::from(in_inbox))
    .bind(now_string())
    .bind(account_id)
    .bind(gmail_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn local_set_unread(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
    is_unread: bool,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE gmail_messages SET is_unread = ?, updated_at = ? \
         WHERE account_id = ? AND gmail_id = ?",
    )
    .bind(i64::from(is_unread))
    .bind(now_string())
    .bind(account_id)
    .bind(gmail_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
pub(crate) struct OutboxRow {
    pub id: i64,
    pub account_id: String,
    pub gmail_id: String,
    pub kind: String,
    pub attempts: i64,
}

pub(crate) async fn enqueue_outbox(
    pool: &SqlitePool,
    account_id: &str,
    gmail_id: &str,
    kind: &str,
) -> Result<(), Error> {
    let now = now_string();
    sqlx::query(
        "INSERT OR IGNORE INTO gmail_outbox \
         (account_id, gmail_id, kind, next_attempt_at, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(gmail_id)
    .bind(kind)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn ready_outbox(pool: &SqlitePool) -> Result<Vec<OutboxRow>, Error> {
    Ok(sqlx::query_as::<_, OutboxRow>(
        "SELECT id, account_id, gmail_id, kind, attempts FROM gmail_outbox \
         WHERE completed_at IS NULL AND next_attempt_at <= ? \
         ORDER BY created_at ASC, id ASC",
    )
    .bind(now_string())
    .fetch_all(pool)
    .await?)
}

pub(crate) async fn complete_outbox(pool: &SqlitePool, id: i64) -> Result<(), Error> {
    sqlx::query("UPDATE gmail_outbox SET completed_at = ?, last_error = NULL WHERE id = ?")
        .bind(now_string())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn fail_outbox(
    pool: &SqlitePool,
    id: i64,
    next_attempt_at: &str,
    error: &str,
) -> Result<(), Error> {
    sqlx::query(
        "UPDATE gmail_outbox SET attempts = attempts + 1, next_attempt_at = ?, last_error = ? \
         WHERE id = ? AND completed_at IS NULL",
    )
    .bind(next_attempt_at)
    .bind(error)
    .bind(id)
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
