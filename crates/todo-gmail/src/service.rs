use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use chrono::{SecondsFormat, TimeDelta, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast};
use todo_core::TodoCore;

use crate::error::Error;
use crate::model::{GmailAccount, MailBody, MailEvent, MailFilter, MailListItem, SyncSummary};
use crate::store::{MessageMeta, OutboxRow};
use crate::tokens::TokenStore;
use crate::{oauth, store};

const INITIAL_QUERY: &str = "-in:trash -in:spam newer_than:90d";
const INITIAL_MAX_RESULTS: u32 = 200;

const DEFAULT_OAUTH_BASE: &str = "https://oauth2.googleapis.com";
const DEFAULT_GMAIL_BASE: &str = "https://gmail.googleapis.com/gmail/v1";

#[derive(Clone)]
pub struct GmailService {
    core: TodoCore,
    client: reqwest::Client,
    tokens: Arc<dyn TokenStore>,
    oauth_base: String,
    gmail_base: String,
    access_cache: Arc<RwLock<HashMap<String, (String, i64)>>>,
    events: broadcast::Sender<MailEvent>,
}

impl GmailService {
    pub fn new(core: TodoCore, tokens: Arc<dyn TokenStore>) -> Self {
        Self::with_endpoints(core, tokens, DEFAULT_OAUTH_BASE, DEFAULT_GMAIL_BASE)
    }

    pub fn with_endpoints(
        core: TodoCore,
        tokens: Arc<dyn TokenStore>,
        oauth_base: impl Into<String>,
        gmail_base: impl Into<String>,
    ) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            core,
            client: reqwest::Client::new(),
            tokens,
            oauth_base: oauth_base.into(),
            gmail_base: gmail_base.into(),
            access_cache: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    pub async fn set_client_credentials(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), Error> {
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            return Err(Error::InvalidInput(
                "client id and secret must not be blank".to_owned(),
            ));
        }
        store::set_setting(self.core.pool(), "gmail.client_id", client_id).await?;
        store::set_setting(self.core.pool(), "gmail.client_secret", client_secret).await?;
        Ok(())
    }

    pub async fn client_id(&self) -> Result<Option<String>, Error> {
        store::get_setting(self.core.pool(), "gmail.client_id").await
    }

    /// OAuth 콜백에서 받은 code 를 토큰으로 교환하고 계정을 등록한다.
    pub async fn complete_auth(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<GmailAccount, Error> {
        let (client_id, client_secret) = self.require_credentials().await?;
        let token = oauth::exchange_code(
            &self.client,
            &self.oauth_base,
            &client_id,
            &client_secret,
            code,
            verifier,
            redirect_uri,
        )
        .await?;
        let refresh = token.refresh_token.clone().ok_or_else(|| {
            Error::InvalidInput(
                "Google did not return a refresh token; retry with consent".to_owned(),
            )
        })?;
        let profile =
            oauth::fetch_profile(&self.client, &self.gmail_base, &token.access_token).await?;
        self.tokens.set(&profile.email, &refresh)?;
        let account = store::insert_account(self.core.pool(), &profile.email).await?;
        store::set_history_id(self.core.pool(), &account.id, &profile.history_id).await?;
        self.cache_access_token(&profile.email, &token.access_token, token.expires_in)
            .await;
        self.emit();
        store::fetch_account(self.core.pool(), &account.id).await
    }

    /// 계정의 캐시된 메일을 최신화한다. history_id 가 있으면 증분, 없거나 만료면 초기.
    pub async fn sync_account(&self, account_id: &str) -> Result<SyncSummary, Error> {
        let account = store::fetch_account(self.core.pool(), account_id).await?;
        if account.history_id.is_some() {
            match self.incremental_sync(&account).await {
                Err(Error::HistoryExpired) => self.initial_sync(&account).await,
                other => other,
            }
        } else {
            self.initial_sync(&account).await
        }
    }

    async fn incremental_sync(&self, account: &GmailAccount) -> Result<SyncSummary, Error> {
        let token = self.access_token(&account.email).await?;
        let start = account
            .history_id
            .as_deref()
            .ok_or(Error::HistoryExpired)?;
        let url = format!(
            "{}/users/me/history?startHistoryId={}\
             &historyTypes=messageAdded&historyTypes=messageDeleted\
             &historyTypes=labelAdded&historyTypes=labelRemoved",
            self.gmail_base, start,
        );
        let response = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| Error::Remote {
                message: "could not reach Gmail".to_owned(),
                retryable: true,
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::HistoryExpired);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            return Err(Error::Remote {
                message: format!("Gmail history returned HTTP {status}"),
                retryable: status.is_server_error()
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
            });
        }
        let body: HistoryResponse = response.json().await.map_err(|_| Error::Remote {
            message: "Gmail history returned an invalid response".to_owned(),
            retryable: true,
        })?;
        let pool = self.core.pool();
        let mut summary = SyncSummary::default();
        for record in body.history {
            for added in record.messages_added {
                let meta = self.fetch_message_meta(&token, &added.message.id).await?;
                store::upsert_message_meta(pool, &account.id, &meta).await?;
                summary.fetched += 1;
                summary.updated += 1;
            }
            for deleted in record.messages_deleted {
                store::delete_message(pool, &account.id, &deleted.message.id).await?;
                summary.updated += 1;
            }
            for change in record.labels_added {
                store::apply_label_change(pool, &account.id, &change.message.id, &change.label_ids, true).await?;
                summary.updated += 1;
            }
            for change in record.labels_removed {
                store::apply_label_change(pool, &account.id, &change.message.id, &change.label_ids, false).await?;
                summary.updated += 1;
            }
        }
        if let Some(history_id) = body.history_id {
            store::set_history_id(pool, &account.id, &history_id).await?;
        }
        self.emit();
        Ok(summary)
    }

    async fn initial_sync(&self, account: &GmailAccount) -> Result<SyncSummary, Error> {
        let token = self.access_token(&account.email).await?;
        let url = format!(
            "{}/users/me/messages?q={}&maxResults={}",
            self.gmail_base,
            urlencode(INITIAL_QUERY),
            INITIAL_MAX_RESULTS,
        );
        let list: MessagesList = self.get_json(&token, &url).await?;
        let mut summary = SyncSummary::default();
        for (index, reference) in list.messages.iter().enumerate() {
            let meta = self.fetch_message_meta(&token, &reference.id).await?;
            store::upsert_message_meta(self.core.pool(), &account.id, &meta).await?;
            summary.fetched += 1;
            summary.updated += 1;
            if index % 20 == 19 {
                self.emit();
            }
        }
        let profile =
            oauth::fetch_profile(&self.client, &self.gmail_base, &token).await?;
        store::set_history_id(self.core.pool(), &account.id, &profile.history_id).await?;
        self.emit();
        Ok(summary)
    }

    /// 로컬 캐시에서 폴더·계정·검색 필터로 메일 목록을 조회한다.
    pub async fn list(&self, filter: MailFilter) -> Result<Vec<MailListItem>, Error> {
        store::query_messages(self.core.pool(), &filter).await
    }

    /// 보관: inbox 에서 뺀다. 로컬을 즉시 반영하고 Gmail 반영은 아웃박스로 미룬다.
    pub async fn archive(&self, account_id: &str, gmail_id: &str) -> Result<(), Error> {
        let pool = self.core.pool();
        store::local_set_inbox(pool, account_id, gmail_id, false).await?;
        store::enqueue_outbox(pool, account_id, gmail_id, "archive").await?;
        self.emit();
        Ok(())
    }

    /// 읽음/안읽음 토글. 로컬을 즉시 반영하고 Gmail 반영은 아웃박스로 미룬다.
    pub async fn set_read(
        &self,
        account_id: &str,
        gmail_id: &str,
        read: bool,
    ) -> Result<(), Error> {
        let pool = self.core.pool();
        store::local_set_unread(pool, account_id, gmail_id, !read).await?;
        let kind = if read { "mark_read" } else { "mark_unread" };
        store::enqueue_outbox(pool, account_id, gmail_id, kind).await?;
        self.emit();
        Ok(())
    }

    /// 아웃박스에 쌓인 라벨 변경을 Gmail 에 반영한다.
    pub async fn process_outbox_once(&self) -> Result<(), Error> {
        let pool = self.core.pool();
        let rows = store::ready_outbox(pool).await?;
        for row in rows {
            let account = match store::fetch_account(pool, &row.account_id).await {
                Ok(account) => account,
                // 계정이 사라졌으면 이 항목은 의미가 없다.
                Err(_) => {
                    store::complete_outbox(pool, row.id).await?;
                    continue;
                }
            };
            let token = match self.access_token(&account.email).await {
                Ok(token) => token,
                // 토큰 문제는 재인증 후 재시도한다. 롤백하지 않는다.
                Err(_) => {
                    let next = next_attempt_at(row.attempts + 1);
                    store::fail_outbox(pool, row.id, &next, "authentication required").await?;
                    continue;
                }
            };
            match self.push_modify(&token, &row).await {
                Ok(()) => store::complete_outbox(pool, row.id).await?,
                Err(error) if error.is_retryable() => {
                    let next = next_attempt_at(row.attempts + 1);
                    store::fail_outbox(pool, row.id, &next, &error.safe_message()).await?;
                }
                Err(_) => {
                    // 비재시도 실패: 로컬을 되돌리고 항목을 종료한다.
                    self.rollback_local(&row).await?;
                    store::complete_outbox(pool, row.id).await?;
                    self.emit();
                }
            }
        }
        Ok(())
    }

    async fn push_modify(&self, token: &str, row: &OutboxRow) -> Result<(), Error> {
        let body: Value = match row.kind.as_str() {
            "archive" => json!({ "removeLabelIds": ["INBOX"] }),
            "mark_read" => json!({ "removeLabelIds": ["UNREAD"] }),
            "mark_unread" => json!({ "addLabelIds": ["UNREAD"] }),
            other => {
                return Err(Error::InvalidInput(format!("unknown outbox kind: {other}")));
            }
        };
        let url = format!("{}/users/me/messages/{}/modify", self.gmail_base, row.gmail_id);
        let response = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::Remote {
                message: "could not reach Gmail".to_owned(),
                retryable: true,
            })?;
        let status = response.status();
        // 원격에서 이미 사라진 메시지는 반영할 것이 없으니 성공으로 본다.
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        // 토큰 거부는 재인증 후 재시도해야 하므로 재시도 대상으로 둔다.
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Remote {
                message: "Gmail rejected the token".to_owned(),
                retryable: true,
            });
        }
        if !status.is_success() {
            return Err(Error::Remote {
                message: format!("Gmail modify returned HTTP {status}"),
                retryable: status.is_server_error()
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
            });
        }
        Ok(())
    }

    async fn rollback_local(&self, row: &OutboxRow) -> Result<(), Error> {
        let pool = self.core.pool();
        match row.kind.as_str() {
            "archive" => store::local_set_inbox(pool, &row.account_id, &row.gmail_id, true).await?,
            "mark_read" => {
                store::local_set_unread(pool, &row.account_id, &row.gmail_id, true).await?
            }
            "mark_unread" => {
                store::local_set_unread(pool, &row.account_id, &row.gmail_id, false).await?
            }
            _ => {}
        }
        Ok(())
    }

    /// 본문을 반환한다. 캐시에 있으면 즉시, 없으면 Gmail 에서 페치해 캐시한다.
    pub async fn get_body(&self, account_id: &str, gmail_id: &str) -> Result<MailBody, Error> {
        let pool = self.core.pool();
        if let Some(body) = store::read_body(pool, account_id, gmail_id).await? {
            return Ok(body);
        }
        let account = store::fetch_account(pool, account_id).await?;
        let token = self.access_token(&account.email).await?;
        let (text, html) = self.fetch_body(&token, gmail_id).await?;
        store::write_body(pool, account_id, gmail_id, text.as_deref(), html.as_deref()).await?;
        Ok(MailBody {
            gmail_id: gmail_id.to_owned(),
            body_text: text,
            body_html: html,
        })
    }

    /// 커서 주변 메일 본문을 미리 당긴다. 개별 실패는 무시하고 진행한다.
    pub async fn prefetch_bodies(
        &self,
        account_id: &str,
        gmail_ids: &[String],
    ) -> Result<(), Error> {
        for gmail_id in gmail_ids {
            let _ = self.get_body(account_id, gmail_id).await;
        }
        Ok(())
    }

    async fn fetch_body(
        &self,
        token: &str,
        gmail_id: &str,
    ) -> Result<(Option<String>, Option<String>), Error> {
        let url = format!(
            "{}/users/me/messages/{}?format=full",
            self.gmail_base, gmail_id,
        );
        let message: FullMessage = self.get_json(token, &url).await?;
        let mut text = None;
        let mut html = None;
        if let Some(payload) = &message.payload {
            walk_part(payload, &mut text, &mut html);
        }
        Ok((text, html))
    }

    async fn fetch_message_meta(&self, token: &str, id: &str) -> Result<MessageMeta, Error> {
        let url = format!(
            "{}/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject",
            self.gmail_base, id,
        );
        let message: GmailMessage = self.get_json(token, &url).await?;
        Ok(message.into_meta())
    }

    async fn access_token(&self, email: &str) -> Result<String, Error> {
        let now = Utc::now().timestamp();
        if let Some((token, expiry)) = self.access_cache.read().await.get(email).cloned() {
            if expiry > now {
                return Ok(token);
            }
        }
        let (client_id, client_secret) = self.require_credentials().await?;
        let refresh = self.tokens.get(email)?.ok_or(Error::NotConfigured)?;
        match oauth::refresh_token(
            &self.client,
            &self.oauth_base,
            &client_id,
            &client_secret,
            &refresh,
        )
        .await
        {
            Ok(response) => {
                self.cache_access_token(email, &response.access_token, response.expires_in)
                    .await;
                Ok(response.access_token)
            }
            Err(Error::Unauthorized) => {
                let _ = store::mark_needs_auth(self.core.pool(), email).await;
                Err(Error::Unauthorized)
            }
            Err(other) => Err(other),
        }
    }

    async fn get_json<T: DeserializeOwned>(&self, token: &str, url: &str) -> Result<T, Error> {
        let response = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| Error::Remote {
                message: "could not reach Gmail".to_owned(),
                retryable: true,
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            return Err(Error::Remote {
                message: format!("Gmail returned HTTP {status}"),
                retryable: status.is_server_error()
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
            });
        }
        response.json().await.map_err(|_| Error::Remote {
            message: "Gmail returned an invalid response".to_owned(),
            retryable: true,
        })
    }

    async fn require_credentials(&self) -> Result<(String, String), Error> {
        let id = store::get_setting(self.core.pool(), "gmail.client_id")
            .await?
            .ok_or(Error::NotConfigured)?;
        let secret = store::get_setting(self.core.pool(), "gmail.client_secret")
            .await?
            .ok_or(Error::NotConfigured)?;
        Ok((id, secret))
    }

    async fn cache_access_token(&self, email: &str, token: &str, expires_in: i64) {
        let expiry = Utc::now().timestamp() + expires_in - 60;
        self.access_cache
            .write()
            .await
            .insert(email.to_owned(), (token.to_owned(), expiry));
    }

    fn emit(&self) {
        let _ = self.events.send(MailEvent::Changed);
    }
}

/// attempt 회차에 맞춘 재시도 시각. Linear 아웃박스와 같은 백오프 곡선.
fn next_attempt_at(attempt: i64) -> String {
    let seconds = match attempt {
        i64::MIN..=1 => 5,
        2 => 15,
        3 => 60,
        4 => 300,
        _ => 1_800,
    };
    (Utc::now() + TimeDelta::seconds(seconds)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// `Name <addr@host>` 또는 `addr@host` 형식에서 이름과 주소를 분리한다.
fn parse_from(value: &str) -> (String, String) {
    let value = value.trim();
    if let Some(start) = value.rfind('<') {
        if let Some(end) = value[start..].find('>') {
            let email = value[start + 1..start + end].trim().to_owned();
            let name = value[..start].trim().trim_matches('"').trim().to_owned();
            return (name, email);
        }
    }
    (String::new(), value.to_owned())
}

#[derive(Deserialize)]
struct MessagesList {
    #[serde(default)]
    messages: Vec<MessageRef>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailMessage {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    internal_date: String,
    #[serde(default)]
    payload: Option<Payload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryResponse {
    #[serde(default)]
    history: Vec<HistoryRecord>,
    #[serde(default)]
    history_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryRecord {
    #[serde(default)]
    messages_added: Vec<MessageEnvelope>,
    #[serde(default)]
    messages_deleted: Vec<MessageEnvelope>,
    #[serde(default)]
    labels_added: Vec<LabelChange>,
    #[serde(default)]
    labels_removed: Vec<LabelChange>,
}

#[derive(Deserialize)]
struct MessageEnvelope {
    message: MessageRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LabelChange {
    message: MessageRef,
    #[serde(default)]
    label_ids: Vec<String>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    headers: Vec<Header>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct FullMessage {
    #[serde(default)]
    payload: Option<BodyPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyPart {
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    body: Option<PartBody>,
    #[serde(default)]
    parts: Vec<BodyPart>,
}

#[derive(Deserialize)]
struct PartBody {
    #[serde(default)]
    data: Option<String>,
}

/// MIME 트리를 순회해 첫 text/plain·text/html 본문을 채운다.
fn walk_part(part: &BodyPart, text: &mut Option<String>, html: &mut Option<String>) {
    if part.mime_type == "text/plain" && text.is_none() {
        if let Some(body) = &part.body {
            if let Some(data) = &body.data {
                *text = decode_base64url(data);
            }
        }
    } else if part.mime_type == "text/html" && html.is_none() {
        if let Some(body) = &part.body {
            if let Some(data) = &body.data {
                *html = decode_base64url(data);
            }
        }
    }
    for child in &part.parts {
        walk_part(child, text, html);
    }
}

fn decode_base64url(data: &str) -> Option<String> {
    let cleaned: String = data.split_whitespace().collect();
    let bytes = URL_SAFE_NO_PAD
        .decode(cleaned.trim_end_matches('='))
        .or_else(|_| URL_SAFE.decode(&cleaned))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

impl GmailMessage {
    fn header(&self, name: &str) -> Option<&str> {
        self.payload.as_ref().and_then(|payload| {
            payload
                .headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case(name))
                .map(|header| header.value.as_str())
        })
    }

    fn into_meta(self) -> MessageMeta {
        let from = self.header("From").unwrap_or_default().to_owned();
        let (from_name, from_email) = parse_from(&from);
        let subject = self.header("Subject").unwrap_or_default().to_owned();
        let internal_date = self.internal_date.parse::<i64>().unwrap_or(0);
        let in_inbox = self.label_ids.iter().any(|label| label == "INBOX");
        let is_unread = self.label_ids.iter().any(|label| label == "UNREAD");
        MessageMeta {
            gmail_id: self.id,
            thread_id: self.thread_id,
            from_name,
            from_email,
            subject,
            snippet: self.snippet,
            internal_date,
            in_inbox,
            is_unread,
        }
    }
}
