# Gmail 이메일 탭 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** todoapp에 구글 다계정 이메일 탭을 추가한다. inbox/archive/all 보기, 본문 보기, `e` 보관, 읽음/안읽음 토글을 키보드 중심으로 제공하고, 로컬 캐시 우선 + 증분 동기화로 빠른 체감을 준다.

**Architecture:** 새 크레이트 `todo-gmail`(HTTP·동기화·아웃박스, wiremock으로 단위 테스트)이 SQLite 캐시에 메일을 담는다. `src-tauri`가 OAuth loopback과 IPC 커맨드로 이 크레이트를 데스크톱에 배선하고 `mail:changed` 이벤트를 프론트로 흘린다. Solid 프론트는 로컬 캐시를 즉시 렌더하고 이벤트로 스트리밍 갱신한다.

**Tech Stack:** Rust(sqlx, reqwest 0.13 + rustls, tokio, keyring 3, base64, sha2, rand, thiserror), wiremock 0.6 테스트, Tauri v2, Solid.js + Vitest.

## Global Constraints

- Rust edition = `2024` (모든 크레이트 동일).
- reqwest = `0.13`, `default-features = false`, features `["json", "rustls"]` (Linear 크레이트와 동일).
- sqlx = `0.8`, `default-features = false`, features `["runtime-tokio", "sqlite"]`.
- 마이그레이션은 전부 `crates/todo-core/migrations`에 둔다. `TodoCore::connect`가 `sqlx::migrate!("./migrations")`로 실행한다. 기존 파일은 수정 금지, 새 파일만 추가.
- OAuth 스코프는 `https://www.googleapis.com/auth/gmail.modify` 하나.
- refresh 토큰은 계정 이메일별 키체인 항목(`service="todo"`, `user="gmail-refresh:{email}"`)에 보관. client id/secret은 `settings` 테이블(`gmail.client_id`, `gmail.client_secret`).
- 시각 문자열은 `Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)` 형식(기존 코드와 동일).
- 커밋 메시지 말미에 반드시:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- 응답·주석은 한국어(합니다체), 코드·식별자는 영어.
- 메일 기능은 v1에서 Tauri(데스크톱) 경로만 지원한다. REST(`todo-server`)에는 노출하지 않는다.
- 설계 판단(스펙 §11 열린 항목 해소): `KeyStore`를 이관하지 않는다. `todo-gmail`이 계정 이메일로 키를 잡는 별도 `TokenStore` 트레이트를 정의한다. `todo-linear`는 건드리지 않는다.

---

## File Structure

**새로 만들 파일**

- `crates/todo-gmail/Cargo.toml` — 크레이트 매니페스트.
- `crates/todo-gmail/src/lib.rs` — 공개 API 재노출 + 모듈 선언.
- `crates/todo-gmail/src/error.rs` — `Error`, `TokenStoreError`.
- `crates/todo-gmail/src/model.rs` — `GmailAccount`, `MailListItem`, `MailBody`, `MailFolder`, `MailFilter`, `MailEvent`, `SyncSummary`.
- `crates/todo-gmail/src/tokens.rs` — `TokenStore` 트레이트 + `SystemTokenStore`(keyring).
- `crates/todo-gmail/src/oauth.rs` — PKCE, auth URL 빌드, 토큰 교환/갱신, 프로필 조회.
- `crates/todo-gmail/src/service.rs` — `GmailService`(계정·동기화·본문·아웃박스·워커).
- `crates/todo-gmail/src/store.rs` — SQL 헬퍼(계정/메시지/아웃박스 upsert·조회).
- `crates/todo-gmail/tests/gmail.rs` — 통합 테스트(wiremock 하네스).
- `crates/todo-core/migrations/20260711000000_gmail.sql` — gmail 테이블.
- `src/mail/domain.ts` — 프론트 메일 타입.
- `src/mail/client.ts` — `GmailClient` 인터페이스 + `TauriGmailClient` + 디코더.
- `src/mail/client.test.ts` — 디코더 테스트.
- `src/mail/MailView.tsx` — 메일 탭 뷰 컴포넌트.
- `src/mail/keyboard-mail.test.ts` — mail 스코프 단축키 테스트(신규 파일; 기존 keyboard 테스트와 분리).

**수정할 파일**

- `Cargo.toml`(워크스페이스 루트) — 멤버에 `crates/todo-gmail` 추가.
- `src-tauri/Cargo.toml` — `todo-gmail`, `open`, `sha2`, `base64`, `rand` 의존성 추가.
- `src-tauri/src/lib.rs` — `ShellState`에 `gmail` 추가, 커맨드·이벤트·워커 배선, OAuth loopback.
- `src/keyboard.ts` — `ShortcutScope`에 `"mail"` 추가, mail 액션 분기.
- `src/App.tsx` — 탭바에 Mail 추가, 탭 스위치, MailView 마운트.

**공개 인터페이스(태스크 간 계약)**

```rust
// todo-gmail model.rs
pub struct GmailAccount {
    pub id: String, pub email: String, pub color: String,
    pub history_id: Option<String>, pub sync_state: String,
    pub last_error: Option<String>, pub last_synced_at: Option<String>,
    pub added_at: String,
}
pub struct MailListItem {
    pub account_id: String, pub account_email: String, pub account_color: String,
    pub gmail_id: String, pub thread_id: String,
    pub from_name: String, pub from_email: String,
    pub subject: String, pub snippet: String,
    pub internal_date: i64, pub in_inbox: bool, pub is_unread: bool,
}
pub struct MailBody { pub gmail_id: String, pub body_text: Option<String>, pub body_html: Option<String> }
pub enum MailFolder { Inbox, Archive, All }
pub struct MailFilter { pub folder: MailFolder, pub account_id: Option<String>, pub query: Option<String>, pub limit: Option<u32> }
pub struct SyncSummary { pub fetched: u64, pub updated: u64 }
#[derive(Clone, Copy)] pub enum MailEvent { Changed }

// todo-gmail service.rs — GmailService 공개 메서드 시그니처
pub fn new(core: TodoCore, tokens: Arc<dyn TokenStore>) -> GmailService
pub fn with_endpoints(core: TodoCore, tokens: Arc<dyn TokenStore>, oauth_base: impl Into<String>, gmail_base: impl Into<String>) -> GmailService
pub async fn set_client_credentials(&self, client_id: &str, client_secret: &str) -> Result<(), Error>
pub async fn client_id(&self) -> Result<Option<String>, Error>
pub async fn complete_auth(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<GmailAccount, Error>
pub async fn accounts(&self) -> Result<Vec<GmailAccount>, Error>
pub async fn remove_account(&self, account_id: &str) -> Result<(), Error>
pub async fn list(&self, filter: MailFilter) -> Result<Vec<MailListItem>, Error>
pub async fn get_body(&self, account_id: &str, gmail_id: &str) -> Result<MailBody, Error>
pub async fn prefetch_bodies(&self, account_id: &str, gmail_ids: &[String]) -> Result<(), Error>
pub async fn archive(&self, account_id: &str, gmail_id: &str) -> Result<(), Error>
pub async fn set_read(&self, account_id: &str, gmail_id: &str, read: bool) -> Result<(), Error>
pub async fn sync_account(&self, account_id: &str) -> Result<SyncSummary, Error>
pub async fn sync_all(&self) -> Result<(), Error>
pub async fn process_outbox_once(&self) -> Result<(), Error>
pub fn spawn_workers(&self) -> ()
pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<MailEvent>

// todo-gmail oauth.rs — 순수/HTTP 헬퍼
pub fn generate_pkce() -> (String, String)          // (verifier, challenge)
pub fn random_state() -> String
pub fn build_auth_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String

// todo-gmail tokens.rs
pub trait TokenStore: Send + Sync {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError>;
    fn set(&self, email: &str, refresh_token: &str) -> Result<(), TokenStoreError>;
    fn delete(&self, email: &str) -> Result<(), TokenStoreError>;
}
pub struct SystemTokenStore;   // keyring 기반
```

```typescript
// src/mail/domain.ts
export type MailFolder = "inbox" | "archive" | "all";
export interface GmailAccount { id: string; email: string; color: string; sync_state: string; last_error: string | null; }
export interface MailListItem {
  account_id: string; account_email: string; account_color: string;
  gmail_id: string; thread_id: string; from_name: string; from_email: string;
  subject: string; snippet: string; internal_date: number; in_inbox: boolean; is_unread: boolean;
}
export interface MailBody { gmail_id: string; body_text: string | null; body_html: string | null; }
export interface MailFilter { folder: MailFolder; account_id?: string; q?: string; limit?: number; }

// src/mail/client.ts
export interface GmailClient {
  accounts(): Promise<GmailAccount[]>;
  list(filter: MailFilter): Promise<MailListItem[]>;
  getBody(accountId: string, gmailId: string): Promise<MailBody>;
  archive(accountId: string, gmailId: string): Promise<void>;
  setRead(accountId: string, gmailId: string, read: boolean): Promise<void>;
  sync(): Promise<void>;
  addAccount(): Promise<GmailAccount>;
  removeAccount(accountId: string): Promise<void>;
  subscribe(onChange: () => void): () => void;
}
```

---

## Phase A — `todo-gmail` 크레이트 (백엔드 코어)

### Task 1: 크레이트 스캐폴드 + 에러 + 토큰 스토어

**Files:**
- Create: `crates/todo-gmail/Cargo.toml`, `crates/todo-gmail/src/lib.rs`, `crates/todo-gmail/src/error.rs`, `crates/todo-gmail/src/tokens.rs`
- Modify: `Cargo.toml`(루트 workspace members)

**Interfaces:**
- Produces: `Error`, `TokenStoreError`, `TokenStore` 트레이트, `SystemTokenStore`.

- [ ] **Step 1: 루트 워크스페이스에 멤버 추가**

`Cargo.toml`:
```toml
[workspace]
members = ["crates/todo-core", "crates/todo-linear", "crates/todo-server", "crates/todo-gmail"]
resolver = "2"
```

- [ ] **Step 2: 크레이트 매니페스트**

`crates/todo-gmail/Cargo.toml`:
```toml
[package]
name = "todo-gmail"
version = "0.1.0"
edition = "2024"

[dependencies]
base64 = "0.22"
chrono = "0.4"
keyring = { version = "3", features = ["apple-native", "linux-native", "windows-native"] }
rand = "0.8"
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite"] }
thiserror = "2"
todo-core = { path = "../todo-core" }
tokio = { version = "1", features = ["sync", "time"] }

[dev-dependencies]
tempfile = "3"
wiremock = "0.6"
```

- [ ] **Step 3: 에러 타입**

`crates/todo-gmail/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
#[error("token store operation failed")]
pub struct TokenStoreError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Gmail is not configured")]
    NotConfigured,
    #[error("Google rejected the credentials")]
    Unauthorized,
    #[error("account was not found")]
    AccountNotFound,
    #[error("message was not found")]
    MessageNotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("history is too old to replay")]
    HistoryExpired,
    #[error("{message}")]
    Remote { message: String, retryable: bool },
    #[error("database error: {0}")]
    Database(String),
    #[error(transparent)]
    TokenStore(#[from] TokenStoreError),
}

impl Error {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Remote { retryable: true, .. })
    }
    pub(crate) fn safe_message(&self) -> String {
        match self {
            Self::Remote { message, .. } => message.clone(),
            other => other.to_string(),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<todo_core::Error> for Error {
    fn from(error: todo_core::Error) -> Self {
        Self::Database(error.to_string())
    }
}
```

- [ ] **Step 4: 토큰 스토어 (테스트 먼저)**

`crates/todo-gmail/src/tokens.rs`:
```rust
use crate::error::TokenStoreError;

const KEYRING_SERVICE: &str = "todo";

pub trait TokenStore: Send + Sync {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError>;
    fn set(&self, email: &str, refresh_token: &str) -> Result<(), TokenStoreError>;
    fn delete(&self, email: &str) -> Result<(), TokenStoreError>;
}

#[derive(Debug, Default)]
pub struct SystemTokenStore;

impl SystemTokenStore {
    fn entry(email: &str) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(KEYRING_SERVICE, &format!("gmail-refresh:{email}"))
            .map_err(|_| TokenStoreError)
    }
}

impl TokenStore for SystemTokenStore {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError> {
        match Self::entry(email)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(TokenStoreError),
        }
    }
    fn set(&self, email: &str, refresh_token: &str) -> Result<(), TokenStoreError> {
        Self::entry(email)?.set_password(refresh_token).map_err(|_| TokenStoreError)
    }
    fn delete(&self, email: &str) -> Result<(), TokenStoreError> {
        match Self::entry(email)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(TokenStoreError),
        }
    }
}
```

- [ ] **Step 5: lib.rs 모듈 선언(임시)**

`crates/todo-gmail/src/lib.rs`:
```rust
mod error;
mod tokens;

pub use error::{Error, TokenStoreError};
pub use tokens::{SystemTokenStore, TokenStore};
```

- [ ] **Step 6: 빌드 확인 후 커밋**

Run: `cargo build -p todo-gmail`
Expected: 성공.
```bash
git add Cargo.toml crates/todo-gmail
git commit -m "feat(gmail): 크레이트 스캐폴드와 TokenStore"
```

### Task 2: DB 마이그레이션

**Files:**
- Create: `crates/todo-core/migrations/20260711000000_gmail.sql`
- Test: `crates/todo-gmail/tests/gmail.rs`(하네스 + 마이그레이션 스모크)

**Interfaces:**
- Produces: `gmail_accounts`, `gmail_messages`, `gmail_outbox` 테이블.

- [ ] **Step 1: 마이그레이션 SQL** — 스펙 §4의 스키마를 그대로 사용한다(`gmail_accounts`, `gmail_messages`, `gmail_outbox`와 인덱스). 파일 내용은 스펙 §4 코드블록과 동일하게 작성.

- [ ] **Step 2: 테스트 하네스 + 스모크 테스트**

`crates/todo-gmail/tests/gmail.rs`:
```rust
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use tempfile::NamedTempFile;
use todo_core::TodoCore;
use todo_gmail::{TokenStore, TokenStoreError};

#[derive(Default)]
struct MemoryTokenStore { map: Mutex<HashMap<String, String>> }
impl TokenStore for MemoryTokenStore {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError> {
        Ok(self.map.lock().unwrap().get(email).cloned())
    }
    fn set(&self, email: &str, token: &str) -> Result<(), TokenStoreError> {
        self.map.lock().unwrap().insert(email.to_owned(), token.to_owned()); Ok(())
    }
    fn delete(&self, email: &str) -> Result<(), TokenStoreError> {
        self.map.lock().unwrap().remove(email); Ok(())
    }
}

async fn connect() -> (NamedTempFile, TodoCore) {
    let database = NamedTempFile::new().expect("create database");
    let core = TodoCore::connect(&format!("sqlite://{}", database.path().display()))
        .await.expect("connect core");
    (database, core)
}

#[tokio::test]
async fn migration_creates_gmail_tables() {
    let (_db, core) = connect().await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
         ('gmail_accounts','gmail_messages','gmail_outbox')",
    ).fetch_one(core.pool()).await.expect("query");
    assert_eq!(count, 3);
    let _ = Arc::new(MemoryTokenStore::default());
}
```

- [ ] **Step 3: 실행 → 통과 확인 → 커밋**

Run: `cargo test -p todo-gmail migration_creates_gmail_tables`
Expected: PASS.
```bash
git add crates/todo-core/migrations crates/todo-gmail/tests/gmail.rs
git commit -m "feat(gmail): DB 마이그레이션과 테스트 하네스"
```

### Task 3: 모델 + 계정 저장소(store.rs)

**Files:**
- Create: `crates/todo-gmail/src/model.rs`, `crates/todo-gmail/src/store.rs`
- Modify: `crates/todo-gmail/src/lib.rs`
- Test: `crates/todo-gmail/tests/gmail.rs`

**Interfaces:**
- Produces: `GmailAccount`/`MailListItem`/`MailBody`/`MailFolder`/`MailFilter`/`MailEvent`/`SyncSummary`(§File Structure 시그니처), `store::{insert_account, list_accounts, delete_account, next_color}`.

- [ ] **Step 1: model.rs** — §File Structure의 구조체/enum 정의. 직렬화가 필요한 타입(`GmailAccount`,`MailListItem`,`MailBody`)은 `#[derive(Debug, Clone, Serialize)]`. `MailFolder`는 내부용(직렬화 불필요).

- [ ] **Step 2: store.rs 계정 CRUD (테스트 먼저)**

색 팔레트는 등록 순서로 순환한다:
```rust
const COLORS: [&str; 6] = ["#268bd2", "#2aa198", "#859900", "#b58900", "#d33682", "#cb4b16"];

pub(crate) fn color_for_index(index: i64) -> String {
    COLORS[(index as usize) % COLORS.len()].to_owned()
}
```
`insert_account(pool, email) -> GmailAccount`(UUID·색·added_at 채워 INSERT, 이미 있으면 기존 반환), `list_accounts(pool) -> Vec<GmailAccount>`(added_at ASC), `delete_account(pool, id)`.

- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn accounts_insert_list_delete_roundtrip() {
    let (_db, core) = connect().await;
    let a = todo_gmail::store::insert_account(core.pool(), "a@x.com").await.unwrap();
    let b = todo_gmail::store::insert_account(core.pool(), "b@x.com").await.unwrap();
    assert_ne!(a.color, b.color);
    let all = todo_gmail::store::list_accounts(core.pool()).await.unwrap();
    assert_eq!(all.len(), 2);
    todo_gmail::store::delete_account(core.pool(), &a.id).await.unwrap();
    assert_eq!(todo_gmail::store::list_accounts(core.pool()).await.unwrap().len(), 1);
}
```
(`store`를 테스트에서 쓰려면 `lib.rs`에서 `pub mod store;`로 노출. 최종 정리 태스크에서 `pub(crate)`로 좁혀도 됨.)

- [ ] **Step 4: 실행 → 통과 → 커밋**

Run: `cargo test -p todo-gmail accounts_insert_list_delete_roundtrip`
```bash
git add crates/todo-gmail
git commit -m "feat(gmail): 모델과 계정 저장소"
```

### Task 4: OAuth 순수 헬퍼(oauth.rs) — PKCE·auth URL

**Files:**
- Create: `crates/todo-gmail/src/oauth.rs`
- Modify: `crates/todo-gmail/src/lib.rs`
- Test: `crates/todo-gmail/tests/gmail.rs`

**Interfaces:**
- Produces: `generate_pkce() -> (String, String)`, `random_state() -> String`, `build_auth_url(...) -> String`.

- [ ] **Step 1: 구현**

`crates/todo-gmail/src/oauth.rs`(순수 부분):
```rust
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn random_state() -> String { random_token(16) }

pub fn generate_pkce() -> (String, String) {
    let verifier = random_token(48);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

pub fn build_auth_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    let query = params.iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>().join("&");
    format!("{AUTH_ENDPOINT}?{query}")
}

fn urlencode(value: &str) -> String {
    value.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}
```

- [ ] **Step 2: 테스트**
```rust
#[test]
fn pkce_challenge_is_sha256_of_verifier() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    let (verifier, challenge) = todo_gmail::oauth::generate_pkce();
    let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, expected);
}

#[test]
fn auth_url_encodes_scope_and_redirect() {
    let url = todo_gmail::oauth::build_auth_url("cid", "http://127.0.0.1:9999", "chal", "st");
    assert!(url.contains("code_challenge=chal"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A9999"));
    assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.modify"));
}
```
(`lib.rs`에 `pub mod oauth;`.)

- [ ] **Step 3: 실행 → 통과 → 커밋**

Run: `cargo test -p todo-gmail pkce_challenge auth_url`
```bash
git add crates/todo-gmail
git commit -m "feat(gmail): PKCE와 OAuth auth URL 빌더"
```

### Task 5: `GmailService` 골격 + 자격증명 + 토큰 교환/갱신

**Files:**
- Create: `crates/todo-gmail/src/service.rs`
- Modify: `crates/todo-gmail/src/lib.rs`, `crates/todo-gmail/src/oauth.rs`(HTTP 부분 추가)
- Test: `crates/todo-gmail/tests/gmail.rs`

**Interfaces:**
- Consumes: `store`, `oauth`, `TokenStore`, `TodoCore`.
- Produces: `GmailService::{new, with_endpoints, set_client_credentials, client_id, complete_auth}`, 내부 `access_token(email)`.

설계 노트:
- `GmailService`는 `core: TodoCore`, `client: reqwest::Client`, `tokens: Arc<dyn TokenStore>`, `oauth_base: String`, `gmail_base: String`, `access_cache: Arc<RwLock<HashMap<String,(String,i64)>>>`(email→(token, 만료 epoch초)), `events: broadcast::Sender<MailEvent>`를 든다.
- 기본 엔드포인트: `oauth_base="https://oauth2.googleapis.com"`, `gmail_base="https://gmail.googleapis.com/gmail/v1"`.
- `set_client_credentials`는 `settings`에 `gmail.client_id`/`gmail.client_secret` upsert(Linear의 `set_setting` 패턴 그대로).

- [ ] **Step 1: oauth.rs에 토큰 교환/갱신/프로필 추가**

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)] pub refresh_token: Option<String>,
    pub expires_in: i64,
}
#[derive(Debug, Deserialize)]
pub struct Profile { #[serde(rename = "emailAddress")] pub email: String, #[serde(rename = "historyId")] pub history_id: String }

pub(crate) async fn exchange_code(
    client: &reqwest::Client, oauth_base: &str,
    client_id: &str, client_secret: &str, code: &str, verifier: &str, redirect_uri: &str,
) -> Result<TokenResponse, crate::Error> {
    post_token(client, oauth_base, &[
        ("grant_type", "authorization_code"), ("code", code), ("code_verifier", verifier),
        ("client_id", client_id), ("client_secret", client_secret), ("redirect_uri", redirect_uri),
    ]).await
}

pub(crate) async fn refresh_token(
    client: &reqwest::Client, oauth_base: &str,
    client_id: &str, client_secret: &str, refresh_token: &str,
) -> Result<TokenResponse, crate::Error> {
    post_token(client, oauth_base, &[
        ("grant_type", "refresh_token"), ("refresh_token", refresh_token),
        ("client_id", client_id), ("client_secret", client_secret),
    ]).await
}

async fn post_token(client: &reqwest::Client, oauth_base: &str, form: &[(&str, &str)]) -> Result<TokenResponse, crate::Error> {
    let response = client.post(format!("{oauth_base}/token")).form(form).send().await
        .map_err(|_| crate::Error::Remote { message: "could not reach Google OAuth".into(), retryable: true })?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST || response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(crate::Error::Unauthorized);
    }
    if !response.status().is_success() {
        return Err(crate::Error::Remote { message: format!("token endpoint HTTP {}", response.status()), retryable: response.status().is_server_error() });
    }
    response.json().await.map_err(|_| crate::Error::Remote { message: "invalid token response".into(), retryable: true })
}
```

- [ ] **Step 2: service.rs — 골격 + credentials + access_token + complete_auth**

핵심 로직(요지):
- `complete_auth(code, verifier, redirect_uri)`: settings에서 client id/secret 로드 → `exchange_code` → `Profile` 조회(`GET {gmail_base}/users/me/profile`, Bearer access_token) → `tokens.set(email, refresh_token)`(refresh_token 없으면 `InvalidInput`) → `store::insert_account(email)` → 계정 `history_id` 저장 → `MailEvent::Changed` emit → 계정 반환.
- `access_token(email)`: 캐시가 유효하면 반환. 아니면 `tokens.get(email)`로 refresh 토큰 로드(없으면 계정 `needs_auth` 표시 후 `NotConfigured`) → `refresh_token()` 호출(`invalid_grant`=`Unauthorized`이면 `needs_auth` 표시) → 캐시에 `(token, now+expires_in-60)` 저장.
- 만료 판정에 필요한 "현재 epoch초"는 `chrono::Utc::now().timestamp()` 사용.

- [ ] **Step 3: 테스트(wiremock)**
```rust
// 하네스: GmailService::with_endpoints(core, tokens, mock.uri(), format!("{}/gmail/v1", mock.uri()))
// - settings에 client id/secret 심고
// - POST /token → { access_token, refresh_token, expires_in } 모킹
// - GET /gmail/v1/users/me/profile → { emailAddress, historyId } 모킹
#[tokio::test]
async fn complete_auth_stores_account_and_refresh_token() { /* 계정 1개 생성, tokens.get(email)=Some 확인 */ }
```
(`lib.rs`에서 `mod service; pub use service::GmailService;` 및 `pub use model::*;` 추가.)

- [ ] **Step 4: 실행 → 통과 → 커밋**
```bash
git add crates/todo-gmail
git commit -m "feat(gmail): GmailService 골격과 OAuth 토큰 교환"
```

### Task 6: 초기 동기화 (messages.list + get metadata)

**Files:** Modify: `crates/todo-gmail/src/service.rs`, `crates/todo-gmail/src/store.rs`; Test: `tests/gmail.rs`

**Interfaces:**
- Produces: `GmailService::sync_account(account_id)`(초기 경로), `store::upsert_message_meta(...)`.

설계 노트:
- `sync_account`: 계정의 `history_id`가 있으면 증분(Task 7), 없거나 만료면 초기 동기화.
- 초기 동기화: `GET {gmail_base}/users/me/messages?q=-in:trash -in:spam newer_than:90d&maxResults=200` → id 목록 → 각 id를 `GET .../messages/{id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject` (v1은 개별 요청; 배치는 후속 최적화) → `store::upsert_message_meta`로 upsert하며 `MailEvent::Changed`를 주기적으로 emit → 마지막에 profile의 `historyId`를 계정에 저장, `SyncSummary` 반환.
- 라벨 파싱: 응답 `labelIds`에 `INBOX` 포함=`in_inbox`, `UNREAD` 포함=`is_unread`. `internalDate`(ms 문자열)→i64. From 헤더에서 이름/주소 분리(`Name <a@b>` 또는 `a@b`).

- [ ] **Step 1: store.rs — upsert_message_meta** (account_id+gmail_id PK로 INSERT ... ON CONFLICT DO UPDATE, 단 본문 컬럼은 건드리지 않음).
- [ ] **Step 2: service.rs — 초기 sync 로직 + From 파서 헬퍼.**
- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn initial_sync_fetches_and_stores_messages() {
    // GET /messages → { messages: [{id:"m1",threadId:"t1"}], resultSizeEstimate:1 } (+ profile)
    // GET /messages/m1?format=metadata → labelIds ["INBOX","UNREAD"], payload.headers From/Subject, internalDate
    // sync_account 후 list(inbox)로 1건, is_unread=true, from 파싱 확인
}
```
- [ ] **Step 4: 커밋** `feat(gmail): 초기 메시지 동기화`

### Task 7: 증분 동기화 (history.list)

**Files:** Modify: `crates/todo-gmail/src/service.rs`; Test: `tests/gmail.rs`

**Interfaces:** Produces: `sync_account` 증분 경로, `store::{set_labels, delete_message}`.

설계 노트:
- `GET {gmail_base}/users/me/history?startHistoryId={hid}&historyTypes=messageAdded&historyTypes=labelAdded&historyTypes=labelRemoved` → `history[]` 순회.
  - `messagesAdded` → 해당 id를 metadata 페치 후 upsert.
  - `labelsAdded`/`labelsRemoved` → `INBOX`/`UNREAD` 변화만 반영(`store::set_labels`).
  - `messagesDeleted` → `store::delete_message`.
- 응답의 최상위 `historyId`로 계정 전진, `MailEvent::Changed` emit.
- HTTP 404(historyId 만료) → `Error::HistoryExpired` → `sync_account`가 초기 동기화로 폴백.

- [ ] **Step 1: store.rs set_labels/delete_message.**
- [ ] **Step 2: service.rs 증분 로직 + 404→HistoryExpired→폴백.**
- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn incremental_sync_applies_label_removal() {
    // 초기: m1 INBOX. 계정 history_id 세팅.
    // GET /history → labelsRemoved INBOX for m1 → list(inbox) 0건, list(archive) 1건.
}
#[tokio::test]
async fn expired_history_falls_back_to_initial_sync() { /* /history 404 → 초기 경로로 채움 */ }
```
- [ ] **Step 4: 커밋** `feat(gmail): history 증분 동기화`

### Task 8: 본문 페치 + prefetch

**Files:** Modify: `crates/todo-gmail/src/service.rs`, `crates/todo-gmail/src/store.rs`; Test: `tests/gmail.rs`

**Interfaces:** Produces: `get_body`, `prefetch_bodies`, `store::{read_body, write_body}`.

설계 노트:
- `get_body(account_id, gmail_id)`: `store::read_body`가 채워져 있으면 반환. 아니면 `GET .../messages/{id}?format=full` → `payload`를 재귀 순회해 `text/plain`·`text/html` 파트의 `body.data`(base64url)를 디코드 → `store::write_body` → 반환.
- `prefetch_bodies(account_id, ids)`: 각 id에 대해 본문이 없으면 위 페치를 수행(에러는 무시하고 진행).
- 멀티파트 파서: `payload.parts`가 있으면 재귀, 없으면 `payload` 자체의 mimeType 확인.

- [ ] **Step 1: store.rs read_body/write_body.**
- [ ] **Step 2: service.rs 본문 페치 + base64url 디코드 + 멀티파트 파서.**
- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn get_body_fetches_and_caches() {
    // GET /messages/m1?format=full → payload.parts[text/plain,text/html] base64url
    // 1회차: body_text/html 디코드 확인. 2회차: 모킹 제거해도(캐시) 성공.
}
```
- [ ] **Step 4: 커밋** `feat(gmail): 본문 지연 로드와 prefetch`

### Task 9: 로컬 목록 조회(list)

**Files:** Modify: `crates/todo-gmail/src/service.rs`, `crates/todo-gmail/src/store.rs`; Test: `tests/gmail.rs`

**Interfaces:** Produces: `GmailService::list(MailFilter)`, `store::query_messages`.

설계 노트:
- 폴더: `Inbox`→`in_inbox=1`, `Archive`→`in_inbox=0`, `All`→조건 없음.
- `account_id` 지정 시 해당 계정만. `query` 지정 시 `subject`/`from_name`/`from_email`/`snippet` LIKE(대소문자 무시). `internal_date DESC` 정렬, `limit` 기본 200.
- 계정 색·이메일은 `gmail_messages JOIN gmail_accounts`로 채운다.

- [ ] **Step 1: store.rs query_messages(QueryBuilder로 필터 조립).**
- [ ] **Step 2: service.rs list 위임.**
- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn list_filters_by_folder_account_and_query() {
    // 2계정, inbox/archive 섞어 upsert 후 각 필터 결과 수 검증.
}
```
- [ ] **Step 4: 커밋** `feat(gmail): 로컬 목록 조회 필터`

### Task 10: 낙관적 쓰기(archive/set_read) + 아웃박스 + 처리

**Files:** Modify: `crates/todo-gmail/src/service.rs`, `crates/todo-gmail/src/store.rs`; Test: `tests/gmail.rs`

**Interfaces:** Produces: `archive`, `set_read`, `process_outbox_once`, `store::{enqueue_outbox, ready_outbox, complete_outbox, fail_outbox, local_apply_label}`.

설계 노트:
- `archive`: 로컬 `in_inbox=0` 즉시 반영 + `MailEvent::Changed` → `enqueue_outbox(kind="archive")`.
- `set_read(read)`: 로컬 `is_unread=!read` 반영 + emit → `enqueue_outbox(kind = if read {"mark_read"} else {"mark_unread"})`.
- `process_outbox_once`: `ready_outbox`(completed_at IS NULL AND next_attempt_at<=now) 순회 → kind별 `POST .../messages/{id}/modify`:
  - archive: `{ "removeLabelIds": ["INBOX"] }`
  - mark_read: `{ "removeLabelIds": ["UNREAD"] }`
  - mark_unread: `{ "addLabelIds": ["UNREAD"] }`
  - 성공 → `complete_outbox`. 재시도 실패 → `fail_outbox`(attempts+1, 백오프 next_attempt_at; Linear의 `backoff_for_attempt` 로직 복제). 비재시도 실패 → 로컬 롤백 + `complete_outbox`로 종료 + emit.
- 백오프: attempt별 5s/15s/60s/300s/1800s(Linear와 동일).

- [ ] **Step 1: store.rs 아웃박스 헬퍼 + local_apply_label.**
- [ ] **Step 2: service.rs archive/set_read/process_outbox_once + backoff.**
- [ ] **Step 3: 테스트**
```rust
#[tokio::test]
async fn archive_optimistic_then_pushes_modify() {
    // archive 호출 직후 list(inbox) 0건(낙관적).
    // POST /messages/m1/modify 모킹(removeLabelIds INBOX 포함 검증) → process_outbox_once → 아웃박스 완료.
}
#[tokio::test]
async fn set_read_enqueues_and_pushes() { /* mark_read modify 검증 */ }
```
- [ ] **Step 4: 커밋** `feat(gmail): 낙관적 보관·읽음과 아웃박스`

### Task 11: 워커 + 이벤트 구독 + sync_all + accounts/remove

**Files:** Modify: `crates/todo-gmail/src/service.rs`, `crates/todo-gmail/src/lib.rs`; Test: `tests/gmail.rs`

**Interfaces:** Produces: `accounts`, `remove_account`, `sync_all`, `subscribe`, `spawn_workers`.

설계 노트:
- `accounts()`→`store::list_accounts`. `remove_account(id)`→계정 이메일로 `tokens.delete` 후 `store::delete_account`(CASCADE로 메시지·아웃박스 정리) + emit.
- `sync_all()`→모든 계정 `sync_account`(에러는 로깅하고 계속).
- `subscribe()`→`events.subscribe()`. `spawn_workers()`→ (a) 60초 주기 `sync_all`, (b) 5초 주기 `process_outbox_once` 두 태스크 spawn(Linear `spawn_worker` 패턴).

- [ ] **Step 1: 구현.**
- [ ] **Step 2: 테스트**
```rust
#[tokio::test]
async fn remove_account_deletes_token_and_messages() {
    // 계정+메시지 생성 → remove_account → list_accounts 0, tokens.get None, list(all) 0.
}
```
- [ ] **Step 3: 전체 크레이트 테스트 → 커밋**

Run: `cargo test -p todo-gmail`
```bash
git add crates/todo-gmail
git commit -m "feat(gmail): 워커·이벤트·계정 관리"
```

### Task 12: 크레이트 마감 정리

**Files:** Modify: `crates/todo-gmail/src/lib.rs`

- [ ] **Step 1:** 공개 표면 정리 — 외부에 필요한 것만 `pub`(`GmailService`, 모델들, `TokenStore`/`SystemTokenStore`/`TokenStoreError`, `Error`). `store`/`oauth`는 통합 테스트가 참조하므로 `pub mod`를 유지하되, 테스트 외 미사용 항목은 `#[allow(dead_code)]` 대신 실제 사용처가 있으니 그대로 둔다.
- [ ] **Step 2:** `cargo clippy -p todo-gmail --all-targets`로 경고 정리.
- [ ] **Step 3: 커밋** `chore(gmail): 공개 표면 정리`

Run: `cargo test -p todo-gmail && cargo clippy -p todo-gmail --all-targets`
Expected: 통과, 경고 없음.

---

## Phase B — 데스크톱 배선 (`src-tauri`)

### Task 13: Tauri 커맨드 + 상태 + 이벤트 배선

**Files:** Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `todo_gmail::{GmailService, SystemTokenStore, MailFilter, MailFolder, ...}`.
- Produces: IPC 커맨드 `gmail_accounts`, `gmail_list`, `gmail_get_body`, `gmail_archive`, `gmail_set_read`, `gmail_sync`, `gmail_remove_account`, `gmail_set_credentials`, Tauri 이벤트 `mail:changed`.

- [ ] **Step 1:** `src-tauri/Cargo.toml`에 의존성 추가:
```toml
todo-gmail = { path = "../crates/todo-gmail" }
open = "5"
base64 = "0.22"
sha2 = "0.10"
rand = "0.8"
```
- [ ] **Step 2:** `ShellState`에 `gmail: GmailService` 추가. `initialize`에서 `GmailService::new(core.clone(), Arc::new(SystemTokenStore))` 생성, `gmail.spawn_workers()` 호출, `forward_mail_events(app.clone(), gmail.subscribe())`로 `MailEvent`→`app.emit("mail:changed", ())` 포워딩(기존 `forward_domain_events`와 동형).
- [ ] **Step 3:** 커맨드 구현. `CommandError`에 `From<todo_gmail::Error>` 추가(코드 매핑: `NotConfigured`→`gmail_not_configured`, `Unauthorized`→`gmail_unauthorized`, `AccountNotFound`/`MessageNotFound`→`not_found`, `Remote`→`gmail_api_error`, 기타→`internal`). 각 커맨드는 `MailFilter` 등 요청 DTO를 받아 서비스에 위임. `gmail_list`의 folder는 문자열("inbox"/"archive"/"all")→`MailFolder` 매핑.
- [ ] **Step 4:** `invoke_handler`에 8개 커맨드 등록(+ Task 14의 `gmail_add_account`).
- [ ] **Step 5: 빌드 확인 → 커밋**

Run: `cargo build -p todo`
```bash
git add src-tauri
git commit -m "feat(tauri): Gmail IPC 커맨드와 mail:changed 이벤트"
```

### Task 14: OAuth loopback (add_account)

**Files:** Modify: `src-tauri/src/lib.rs`

**Interfaces:** Produces: IPC 커맨드 `gmail_add_account() -> GmailAccount`.

설계 노트(수동 검증 태스크 — 브라우저 동의는 자동 테스트 불가):
- `gmail_add_account`:
  1. `gmail.client_id()`가 없으면 `gmail_not_configured` 반환(프론트가 설정 유도).
  2. `std::net::TcpListener::bind("127.0.0.1:0")`로 임시 포트 확보 → `redirect_uri = format!("http://127.0.0.1:{port}")`.
  3. `oauth::generate_pkce()` + `oauth::random_state()` → `oauth::build_auth_url(client_id, redirect_uri, challenge, state)`.
  4. `open::that(auth_url)`로 시스템 브라우저 열기.
  5. `tokio::task::spawn_blocking`에서 리스너 `accept()` 1회 → 첫 요청 라인 파싱해 `code`·`state` 추출 → state 검증 → HTTP 200 "이 창을 닫아도 됩니다" 응답.
  6. `gmail.complete_auth(&code, &verifier, &redirect_uri).await` → 계정 반환. `MailEvent`가 이미 emit되어 프론트가 갱신.
  7. 타임아웃(예: 180초) 초과 시 오류 반환.
- 리스너 요청 파서: 첫 줄 `GET /?code=...&state=... HTTP/1.1`에서 쿼리스트링만 파싱하면 충분.

- [ ] **Step 1:** 위 로직 구현 + `invoke_handler`에 `gmail_add_account` 등록.
- [ ] **Step 2: 빌드 → 커밋**

Run: `cargo build -p todo`
```bash
git add src-tauri
git commit -m "feat(tauri): Gmail OAuth loopback 계정 추가"
```

---

## Phase C — 프론트엔드 (Solid)

### Task 15: 메일 도메인 + 클라이언트 + 디코더

**Files:** Create: `src/mail/domain.ts`, `src/mail/client.ts`, `src/mail/client.test.ts`

**Interfaces:** §File Structure의 `MailFolder`/`GmailAccount`/`MailListItem`/`MailBody`/`MailFilter`/`GmailClient`/`TauriGmailClient`.

설계 노트:
- `src/client.ts`의 방어적 디코더(`isRecord` 등) 스타일을 그대로 따른다. `decodeMailListItem`, `decodeAccount`, `decodeBody`.
- `TauriGmailClient`는 기존 `TauriClient`처럼 `invoke`/`listen`을 주입받는다. `subscribe`는 `mail:changed` 이벤트를 listen. `list`는 `gmail_list`에 `{ filter: { folder, account_id, q, limit } }` 전달. `addAccount`→`gmail_add_account`, `removeAccount`→`gmail_remove_account`, `sync`→`gmail_sync`, `archive`→`gmail_archive`, `setRead`→`gmail_set_read`, `getBody`→`gmail_get_body`.

- [ ] **Step 1:** `domain.ts` 타입 작성.
- [ ] **Step 2:** `client.test.ts` — 디코더 테스트(먼저).
```ts
import { describe, expect, it } from "vitest";
import { decodeMailListItem } from "./client";
describe("decodeMailListItem", () => {
  it("정상 페이로드를 파싱한다", () => {
    const item = decodeMailListItem({ account_id:"a", account_email:"a@x.com", account_color:"#268bd2",
      gmail_id:"m1", thread_id:"t1", from_name:"Kim", from_email:"kim@x.com", subject:"hi",
      snippet:"...", internal_date: 123, in_inbox: true, is_unread: false });
    expect(item.gmail_id).toBe("m1"); expect(item.in_inbox).toBe(true);
  });
  it("필드 누락 시 던진다", () => {
    expect(() => decodeMailListItem({ gmail_id: "m1" })).toThrow();
  });
});
```
- [ ] **Step 3:** `client.ts` 구현 → 테스트 통과.

Run: `pnpm test -- src/mail/client.test.ts`
- [ ] **Step 4: 커밋** `feat(mail): 프론트 도메인·클라이언트`

### Task 16: keyboard.ts mail 스코프

**Files:** Modify: `src/keyboard.ts`; Create: `src/mail/keyboard-mail.test.ts`

**Interfaces:**
- Consumes: 기존 `handleKey`, `KeyboardState`, `Action`.
- Produces: `ShortcutScope`에 `"mail"` 추가, mail 액션(`MailMove`, `MailOpen`, `MailClose`, `MailArchive`, `MailToggleRead`, `MailSetFolder`)과 mail 분기.

설계 노트:
- `handleKey`에서 `topScope === "mail"`일 때 별도 분기. Todo와 상태를 공유하지 않도록 `KeyboardState`에 `mailIndex`/`mailIds`/`mailOpen` 같은 필드를 추가하기보다, **App 쪽에서 mail 전용 최소 상태를 keyboard에 넘기는 대신 mail 뷰가 자체 keydown 핸들러를 갖는** 방식을 택한다(App의 전역 핸들러는 Todo용 유지). 즉 `keyboard.ts`는 Todo 중심으로 두고, **mail 키 처리는 `MailView.tsx` 내부의 순수 함수 `handleMailKey(state, event)`로 구현**한다. 테스트도 그 함수를 대상으로 한다.
- `handleMailKey`는 `j/k`(이동), `Enter`(열기), `Escape`(닫기/해제), `e`(보관), `u`(읽음 토글), `1/2/3`(inbox/archive/all), `/`(검색)만 다루고 `⌘K`/`?`는 전역(App)에서 처리. 텍스트 입력 포커스면 `null`.

`MailView.tsx`에 함께 둘 순수 함수(테스트 대상):
```ts
export type MailKeyAction =
  | { type: "Move"; delta: number }
  | { type: "Open" } | { type: "Close" }
  | { type: "Archive" } | { type: "ToggleRead" }
  | { type: "SetFolder"; folder: MailFolder }
  | { type: "OpenSearch" };
export interface MailKeyState { focus: "text" | "other"; detailOpen: boolean; }
export function handleMailKey(state: MailKeyState, key: string): MailKeyAction | null { /* 위 매핑 */ }
```

- [ ] **Step 1:** `keyboard.ts`의 `ShortcutScope`에 `"mail"` 추가(App 탭 전환에 사용).
- [ ] **Step 2:** `handleMailKey` 테스트(먼저) `src/mail/keyboard-mail.test.ts`.
```ts
import { describe, expect, it } from "vitest";
import { handleMailKey } from "./MailView";
const other = { focus: "other" as const, detailOpen: false };
describe("handleMailKey", () => {
  it("j는 아래로 이동", () => expect(handleMailKey(other, "j")).toEqual({ type:"Move", delta:1 }));
  it("e는 보관", () => expect(handleMailKey(other, "e")).toEqual({ type:"Archive" }));
  it("u는 읽음 토글", () => expect(handleMailKey(other, "u")).toEqual({ type:"ToggleRead" }));
  it("2는 archive 폴더", () => expect(handleMailKey(other, "2")).toEqual({ type:"SetFolder", folder:"archive" }));
  it("텍스트 포커스면 무시", () => expect(handleMailKey({ focus:"text", detailOpen:false }, "e")).toBeNull());
  it("Escape는 상세 열림 시 닫기", () => expect(handleMailKey({ focus:"other", detailOpen:true }, "Escape")).toEqual({ type:"Close" }));
});
```
- [ ] **Step 3:** `MailView.tsx`에 `handleMailKey`만 먼저 구현(컴포넌트는 Task 17) → 테스트 통과.

Run: `pnpm test -- src/mail/keyboard-mail.test.ts`
- [ ] **Step 4: 커밋** `feat(mail): mail 스코프 단축키 리듀서`

### Task 17: MailView 컴포넌트

**Files:** Modify: `src/mail/MailView.tsx`; `src/styles.css`(메일 스타일 추가)

**Interfaces:**
- Consumes: `GmailClient`, `handleMailKey`, `MailListItem`, `MailBody`, `GmailAccount`.
- Produces: `MailView` 컴포넌트(props: `client: GmailClient`).

설계 노트(수동 검증 위주 — 렌더/이벤트 통합):
- 시그널: `accounts`, `messages`, `folder`(기본 inbox), `accountFilter`, `cursor`, `openId`, `body`, `search`, `syncing`.
- `onMount`: `client.list({folder})` 즉시 렌더 → `client.subscribe(reload)` → `client.sync()` 백그라운드 트리거. `folder`/`accountFilter`/`search` 변경 시 `list` 재조회.
- progressive: 목록은 메타만으로 렌더. `cursor` 변경 시 커서±3의 `gmail_id`를 모아 본문 프리페치(`getBody`를 병렬 호출하되 결과는 열 때 사용). 메일 열면 `getBody`로 본문 표시 + `setRead(true)`.
- 각 행: 왼쪽 `border-left: 3px solid {account_color}`, 발신자, 제목, 스니펫, 날짜(`new Date(internal_date)`), 안읽음 볼드/점, 계정 라벨(이메일 로컬파트).
- keydown 핸들러: `handleMailKey`로 액션 산출 → 실행(Move=cursor, Open=openId+getBody+setRead, Close, Archive=client.archive+낙관적 목록 제거, ToggleRead, SetFolder, OpenSearch).
- 상단: 폴더 필터 칩(inbox/archive/all) + 계정 필터 + 동기화 표시 + "계정 추가" 버튼(`client.addAccount()`), `needs_auth` 계정은 재인증 배지.
- 자격증명 미설정(`gmail_not_configured`) 시: client_id/secret 입력 폼 + OAuth 설정 가이드 링크 안내.

- [ ] **Step 1:** 컴포넌트 구현(위 명세대로). 스타일은 기존 `.todo-row`/`.detail-panel` 클래스를 재사용/확장.
- [ ] **Step 2:** 타입 체크 + 빌드.

Run: `pnpm exec tsc --noEmit && pnpm test`
- [ ] **Step 3: 커밋** `feat(mail): MailView 컴포넌트`

### Task 18: App 탭 배선 + 팔레트 명령

**Files:** Modify: `src/App.tsx`, `src/main.tsx`(GmailClient 주입)

**Interfaces:** Consumes: `MailView`, `TauriGmailClient`.

설계 노트:
- `App`에 `activeTab` 시그널(`"todo" | "mail"`). 탭바에 `Mail` 버튼 추가, 클릭·단축키로 전환. `scopeStack`은 `activeTab==="mail"`이면 `["global","mail"]`.
- 전역 `onKeyDown`: mail 탭일 때 Todo 키 처리를 건너뛰고(⌘K/?/탭전환만 전역 처리) 나머지는 `MailView` 내부 핸들러에 맡긴다. 탭 전환 키는 `g` chord 후 `t`/`m`(기존 chord 타이머 재사용) — 구현 부담이면 우선 클릭 + 팔레트 명령으로 두고 chord는 후속.
- 커맨드 팔레트에 `mail`: "메일 열기", "계정 추가", "메일 동기화" 명령 추가.
- `main.tsx`: Tauri 환경에서 `TauriGmailClient` 생성해 `App`에 주입. 비-Tauri(HttpClient) 환경에선 mail 탭 비활성(주입 없으면 탭 숨김).

- [ ] **Step 1:** App 탭 상태·탭바·MailView 마운트·팔레트 명령.
- [ ] **Step 2:** main.tsx 주입.
- [ ] **Step 3:** 타입 체크·테스트·빌드.

Run: `pnpm exec tsc --noEmit && pnpm test && pnpm build`
- [ ] **Step 4: 커밋** `feat(mail): App 탭 배선과 팔레트 명령`

---

## Self-Review

**1. Spec coverage**
- 구글 다계정 로그인 → Task 5(complete_auth), 11(accounts/remove), 14(loopback).
- inbox/archive/all → Task 9(list 폴더), 17(필터 UI).
- 통합 목록 + 계정 색/라벨 → Task 9(JOIN), 17(색 스트라이프·라벨).
- 본문 보기(지연+prefetch) → Task 8, 17.
- `e` 보관, 읽음/안읽음 토글, 열람 읽음 → Task 10, 16, 17.
- progressive(로컬 즉시 + 증분) → Task 6/7(동기화), 17(즉시 렌더+구독).
- 낙관적 쓰기 + 아웃박스 → Task 10.
- 재인증(7일 만료) → Task 5(access_token needs_auth), 17(재인증 배지), 14(재실행).
- 키보드 트리아지 → Task 16, 17, 18.
- OAuth 설정 가이드 → 스펙 §6.4 참조를 Task 17 안내 UI에 링크.

**2. Placeholder scan**: UI/loopback 태스크(13·14·17·18)는 완전 리터럴 대신 정밀 명세 + 핵심 코드로 기술했다(자동 테스트가 어려운 통합/렌더 영역). 알고리즘·계약이 있는 백엔드·디코더·리듀서(1~12·15·16)는 실제 코드와 실패 테스트를 담았다. "TODO/추후" 없음.

**3. Type consistency**: `MailFilter.folder`는 Rust `MailFolder`(enum)↔TS `"inbox"|"archive"|"all"`, 커맨드 경계에서 문자열 매핑(Task 13). `handleMailKey`/`MailKeyAction`은 Task 16 정의를 17에서 그대로 사용. 서비스 시그니처는 §File Structure를 단일 출처로 사용.

**미해결 → 계획 중 확정**: 탭 전환·재인증 키의 chord 매핑은 Task 18에서 "우선 클릭+팔레트, chord는 후속"으로 확정. 초기 동기화 창은 `newer_than:90d`, `maxResults=200`으로 확정(Task 6).

