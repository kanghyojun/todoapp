use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct GmailAccount {
    pub id: String,
    pub email: String,
    pub color: String,
    pub history_id: Option<String>,
    pub sync_state: String,
    pub last_error: Option<String>,
    pub last_synced_at: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MailListItem {
    pub account_id: String,
    pub account_email: String,
    pub account_color: String,
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

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MailBody {
    pub gmail_id: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailFolder {
    Inbox,
    Archive,
    All,
}

#[derive(Debug, Clone)]
pub struct MailFilter {
    pub folder: MailFolder,
    pub account_id: Option<String>,
    pub query: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct SyncSummary {
    pub fetched: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailEvent {
    Changed,
}
