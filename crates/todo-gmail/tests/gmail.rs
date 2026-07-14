use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tempfile::NamedTempFile;
use todo_core::{EmailLinkInput, TodoCore};
use todo_gmail::{GmailService, MailFilter, MailFolder, TokenStore, TokenStoreError};
use wiremock::matchers::{body_string_contains, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct MemoryTokenStore {
    map: Mutex<HashMap<String, String>>,
}

impl TokenStore for MemoryTokenStore {
    fn get(&self, email: &str) -> Result<Option<String>, TokenStoreError> {
        Ok(self.map.lock().unwrap().get(email).cloned())
    }
    fn set(&self, email: &str, token: &str) -> Result<(), TokenStoreError> {
        self.map
            .lock()
            .unwrap()
            .insert(email.to_owned(), token.to_owned());
        Ok(())
    }
    fn delete(&self, email: &str) -> Result<(), TokenStoreError> {
        self.map.lock().unwrap().remove(email);
        Ok(())
    }
}

async fn connect() -> (NamedTempFile, TodoCore) {
    let database = NamedTempFile::new().expect("create database");
    let core = TodoCore::connect(&format!("sqlite://{}", database.path().display()))
        .await
        .expect("connect core");
    (database, core)
}

struct Harness {
    _db: NamedTempFile,
    core: TodoCore,
    mock: MockServer,
    tokens: Arc<MemoryTokenStore>,
    service: GmailService,
}

async fn harness() -> Harness {
    let (db, core) = connect().await;
    let mock = MockServer::start().await;
    let tokens = Arc::new(MemoryTokenStore::default());
    let service = GmailService::with_endpoints(
        core.clone(),
        tokens.clone(),
        mock.uri(),
        format!("{}/gmail/v1", mock.uri()),
    );
    service
        .set_client_credentials("cid", "secret")
        .await
        .expect("seed credentials");
    Harness {
        _db: db,
        core,
        mock,
        tokens,
        service,
    }
}

#[tokio::test]
async fn complete_auth_stores_account_and_refresh_token() {
    let harness = harness().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 3600
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@x.com", "historyId": "12345"
        })))
        .mount(&harness.mock)
        .await;

    let account = harness
        .service
        .complete_auth("code-1", "verifier-1", "http://127.0.0.1:1234")
        .await
        .expect("complete auth");
    assert_eq!(account.email, "me@x.com");
    // 등록 시점엔 history_id 를 심지 않는다. 첫 sync 가 전체 백필을 타야 하기 때문이다.
    // history_id 는 initial_sync 가 끝에서 저장한다.
    assert_eq!(account.history_id, None);
    assert_eq!(
        harness.tokens.get("me@x.com").unwrap().as_deref(),
        Some("rt-1")
    );
    let accounts = todo_gmail::store::list_accounts(harness.core.pool())
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
}

async fn mount_token(mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 3600
        })))
        .mount(mock)
        .await;
}

#[tokio::test]
async fn initial_sync_fetches_and_stores_messages() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [ { "id": "m1", "threadId": "t1" } ],
            "resultSizeEstimate": 1
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m1", "threadId": "t1",
            "labelIds": ["INBOX", "UNREAD"],
            "snippet": "hello there",
            "internalDate": "1700000000000",
            "payload": { "headers": [
                { "name": "From", "value": "Kim <kim@x.com>" },
                { "name": "Subject", "value": "greeting" }
            ] }
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@x.com", "historyId": "999"
        })))
        .mount(&harness.mock)
        .await;

    let summary = harness.service.sync_account(&account.id).await.unwrap();
    assert_eq!(summary.fetched, 1);

    let (gmail_id, subject, in_inbox, is_unread, from_name, from_email): (
        String,
        String,
        i64,
        i64,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT gmail_id, subject, in_inbox, is_unread, from_name, from_email \
         FROM gmail_messages WHERE account_id = ?",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(gmail_id, "m1");
    assert_eq!(subject, "greeting");
    assert_eq!(in_inbox, 1);
    assert_eq!(is_unread, 1);
    assert_eq!(from_name, "Kim");
    assert_eq!(from_email, "kim@x.com");

    let refreshed = todo_gmail::store::fetch_account(harness.core.pool(), &account.id)
        .await
        .unwrap();
    assert_eq!(refreshed.history_id.as_deref(), Some("999"));
}

// 회귀: 계정을 추가(complete_auth)한 직후의 첫 sync 는 전체 백필(initial_sync)을
// 타야 한다. complete_auth 가 history_id 를 미리 심으면 첫 sync 가 incremental 로
// 빠지고, 등록 직후라 변경분이 없어 0건으로 끝나며 기존 메일을 하나도 못 가져온다.
// 실제 Gmail 처럼 /history 는 200 OK + 빈 history 로 응답시켜 그 경로를 재현한다.
#[tokio::test]
async fn add_account_then_first_sync_backfills_existing_mail() {
    let harness = harness().await;
    mount_token(&harness.mock).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@x.com", "historyId": "12345"
        })))
        .mount(&harness.mock)
        .await;
    // 등록 직후라 새 변경분이 없다. Gmail 은 200 에 빈 history 를 준다(404 아님).
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "historyId": "12345"
        })))
        .mount(&harness.mock)
        .await;
    // 계정에 이미 쌓여 있던 메일. initial_sync 만이 이걸 가져온다.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [ { "id": "old1", "threadId": "t1" } ],
            "resultSizeEstimate": 1
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/old1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "old1", "threadId": "t1",
            "labelIds": ["INBOX"],
            "snippet": "an existing mail",
            "internalDate": "1699000000000",
            "payload": { "headers": [
                { "name": "From", "value": "Lee <lee@x.com>" },
                { "name": "Subject", "value": "old subject" }
            ] }
        })))
        .mount(&harness.mock)
        .await;

    let account = harness
        .service
        .complete_auth("code-1", "verifier-1", "http://127.0.0.1:1234")
        .await
        .expect("complete auth");
    harness.service.sync_account(&account.id).await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gmail_messages WHERE account_id = ?")
        .bind(&account.id)
        .fetch_one(harness.core.pool())
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "계정 추가 후 첫 sync 는 기존 메일을 전체 백필해야 한다"
    );
}

async fn seed_message(
    core: &TodoCore,
    account_id: &str,
    gmail_id: &str,
    in_inbox: i64,
    is_unread: i64,
) {
    sqlx::query(
        "INSERT INTO gmail_messages \
         (account_id, gmail_id, thread_id, internal_date, in_inbox, is_unread, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(gmail_id)
    .bind("t1")
    .bind(1_700_000_000_000_i64)
    .bind(in_inbox)
    .bind(is_unread)
    .bind("2026-07-11T00:00:00.000Z")
    .execute(core.pool())
    .await
    .expect("seed message");
}

async fn set_account_history(core: &TodoCore, account_id: &str, history_id: &str) {
    sqlx::query("UPDATE gmail_accounts SET history_id = ? WHERE id = ?")
        .bind(history_id)
        .bind(account_id)
        .execute(core.pool())
        .await
        .expect("set history");
}

async fn in_inbox_flag(core: &TodoCore, account_id: &str, gmail_id: &str) -> i64 {
    sqlx::query_scalar("SELECT in_inbox FROM gmail_messages WHERE account_id = ? AND gmail_id = ?")
        .bind(account_id)
        .bind(gmail_id)
        .fetch_one(core.pool())
        .await
        .expect("read flag")
}

#[tokio::test]
async fn incremental_sync_applies_label_removal() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;
    seed_message(&harness.core, &account.id, "m1", 1, 1).await;
    set_account_history(&harness.core, &account.id, "100").await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                { "id": "100", "labelsRemoved": [
                    { "message": { "id": "m1" }, "labelIds": ["INBOX"] }
                ] }
            ],
            "historyId": "101"
        })))
        .mount(&harness.mock)
        .await;

    harness.service.sync_account(&account.id).await.unwrap();
    assert_eq!(in_inbox_flag(&harness.core, &account.id, "m1").await, 0);

    let refreshed = todo_gmail::store::fetch_account(harness.core.pool(), &account.id)
        .await
        .unwrap();
    assert_eq!(refreshed.history_id.as_deref(), Some("101"));
}

// 회귀: history 의 messagesAdded 에 추가됐다 삭제된 메일(fetch 시 404)이 끼어 있어도
// 그 한 건만 건너뛰고 나머지를 처리하며 history_id 를 전진시켜야 한다. 404 에서 통째로
// 중단되면 history_id 가 고정돼 그 계정은 이후 새 메일을 영영 못 받는다(sync 교착).
#[tokio::test]
async fn incremental_sync_skips_deleted_message_and_advances_history() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    set_account_history(&harness.core, &account.id, "100").await;
    mount_token(&harness.mock).await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                { "id": "100", "messagesAdded": [
                    { "message": { "id": "gone" } },
                    { "message": { "id": "kept" } }
                ] }
            ],
            "historyId": "101"
        })))
        .mount(&harness.mock)
        .await;
    // 추가됐다 삭제된 메일: fetch 하면 404.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&harness.mock)
        .await;
    // 정상 새 메일.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/kept"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "kept", "threadId": "t1", "labelIds": ["INBOX"],
            "snippet": "s", "internalDate": "1700000000002",
            "payload": { "headers": [ { "name": "Subject", "value": "kept subject" } ] }
        })))
        .mount(&harness.mock)
        .await;

    harness.service.sync_account(&account.id).await.unwrap();

    // 정상 메일은 저장되고, 삭제된 메일은 저장되지 않는다.
    let kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gmail_messages WHERE account_id = ? AND gmail_id = 'kept'",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(kept, 1, "삭제된 메일을 건너뛰고 정상 메일은 저장해야 한다");

    // history_id 가 전진해 교착이 풀렸다.
    let refreshed = todo_gmail::store::fetch_account(harness.core.pool(), &account.id)
        .await
        .unwrap();
    assert_eq!(
        refreshed.history_id.as_deref(),
        Some("101"),
        "404 한 건 때문에 history_id 가 고정되면 안 된다"
    );
}

// 회귀: initial_sync(백필)도 목록에 오른 메일이 fetch 시 404 면 그 한 건만 건너뛰고
// 나머지를 저장해야 한다. 백필 도중 404 로 중단되면 계정 첫 동기화가 통째로 실패한다.
#[tokio::test]
async fn initial_sync_skips_deleted_message_and_continues() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [ { "id": "gone", "threadId": "t1" }, { "id": "kept", "threadId": "t2" } ],
            "resultSizeEstimate": 2
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/kept"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "kept", "threadId": "t2", "labelIds": ["INBOX"],
            "snippet": "s", "internalDate": "1700000000003",
            "payload": { "headers": [ { "name": "Subject", "value": "kept" } ] }
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@x.com", "historyId": "999"
        })))
        .mount(&harness.mock)
        .await;

    harness.service.sync_account(&account.id).await.unwrap();

    let kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gmail_messages WHERE account_id = ? AND gmail_id = 'kept'",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(kept, 1, "백필 중 404 를 건너뛰고 정상 메일은 저장해야 한다");

    let refreshed = todo_gmail::store::fetch_account(harness.core.pool(), &account.id)
        .await
        .unwrap();
    assert_eq!(refreshed.history_id.as_deref(), Some("999"));
}

// 회귀: history 응답이 nextPageToken 으로 페이지가 나뉘면 모든 페이지를 따라가
// 변경분을 빠짐없이 처리해야 한다. 첫 페이지만 읽고 최신 historyId 로 점프하면
// 2 페이지 이후 변경분(밀린 메일)을 영구히 잃는다.
#[tokio::test]
async fn incremental_sync_follows_pagination() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    set_account_history(&harness.core, &account.id, "100").await;
    mount_token(&harness.mock).await;

    // 1 페이지: pageToken 없는 요청. nextPageToken 을 준다.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                { "id": "100", "messagesAdded": [ { "message": { "id": "p1" } } ] }
            ],
            "nextPageToken": "PAGE2",
            "historyId": "101"
        })))
        .mount(&harness.mock)
        .await;
    // 2 페이지: pageToken=PAGE2. 마지막 페이지.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .and(query_param("pageToken", "PAGE2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                { "id": "101", "messagesAdded": [ { "message": { "id": "p2" } } ] }
            ],
            "historyId": "101"
        })))
        .mount(&harness.mock)
        .await;
    for id in ["p1", "p2"] {
        Mock::given(method("GET"))
            .and(path(format!("/gmail/v1/users/me/messages/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": id, "threadId": "t1", "labelIds": ["INBOX"],
                "snippet": "s", "internalDate": "1700000000004",
                "payload": { "headers": [ { "name": "Subject", "value": id } ] }
            })))
            .mount(&harness.mock)
            .await;
    }

    harness.service.sync_account(&account.id).await.unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gmail_messages WHERE account_id = ? AND gmail_id IN ('p1','p2')",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(count, 2, "모든 history 페이지의 새 메일을 저장해야 한다");

    let refreshed = todo_gmail::store::fetch_account(harness.core.pool(), &account.id)
        .await
        .unwrap();
    assert_eq!(refreshed.history_id.as_deref(), Some("101"));
}

#[tokio::test]
async fn expired_history_falls_back_to_initial_sync() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    set_account_history(&harness.core, &account.id, "5").await;
    mount_token(&harness.mock).await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [ { "id": "m9", "threadId": "t9" } ]
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m9", "threadId": "t9", "labelIds": ["INBOX"],
            "snippet": "s", "internalDate": "1700000000001",
            "payload": { "headers": [ { "name": "Subject", "value": "recovered" } ] }
        })))
        .mount(&harness.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "me@x.com", "historyId": "7"
        })))
        .mount(&harness.mock)
        .await;

    harness.service.sync_account(&account.id).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gmail_messages WHERE account_id = ? AND gmail_id = 'm9'",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn get_body_fetches_and_caches() {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;
    seed_message(&harness.core, &account.id, "m1", 1, 1).await;

    let text_data = URL_SAFE_NO_PAD.encode("Hello world");
    let html_data = URL_SAFE_NO_PAD.encode("<p>hi</p>");
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m1", "threadId": "t1",
            "payload": { "mimeType": "multipart/alternative", "parts": [
                { "mimeType": "text/plain", "body": { "data": text_data } },
                { "mimeType": "text/html", "body": { "data": html_data } }
            ] }
        })))
        .mount(&harness.mock)
        .await;

    let body = harness.service.get_body(&account.id, "m1").await.unwrap();
    assert_eq!(body.body_text.as_deref(), Some("Hello world"));
    assert_eq!(body.body_html.as_deref(), Some("<p>hi</p>"));

    // 본문이 캐시에 저장돼 body_fetched_at 이 채워졌다.
    let fetched: Option<String> = sqlx::query_scalar(
        "SELECT body_fetched_at FROM gmail_messages WHERE account_id = ? AND gmail_id = 'm1'",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert!(fetched.is_some());
}

async fn seed_subject(
    core: &TodoCore,
    account_id: &str,
    gmail_id: &str,
    in_inbox: i64,
    subject: &str,
    internal_date: i64,
) {
    sqlx::query(
        "INSERT INTO gmail_messages \
         (account_id, gmail_id, thread_id, subject, internal_date, in_inbox, is_unread, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 0, ?)",
    )
    .bind(account_id)
    .bind(gmail_id)
    .bind("t1")
    .bind(subject)
    .bind(internal_date)
    .bind(in_inbox)
    .bind("2026-07-11T00:00:00.000Z")
    .execute(core.pool())
    .await
    .expect("seed subject");
}

fn filter(folder: MailFolder, account_id: Option<&str>, query: Option<&str>) -> MailFilter {
    MailFilter {
        folder,
        account_id: account_id.map(str::to_owned),
        query: query.map(str::to_owned),
        limit: None,
    }
}

#[tokio::test]
async fn list_filters_by_folder_account_and_query() {
    let harness = harness().await;
    let a = todo_gmail::store::insert_account(harness.core.pool(), "a@x.com")
        .await
        .unwrap();
    let b = todo_gmail::store::insert_account(harness.core.pool(), "b@x.com")
        .await
        .unwrap();
    seed_subject(&harness.core, &a.id, "m1", 1, "invoice due", 300).await;
    seed_subject(&harness.core, &a.id, "m2", 0, "receipt", 200).await;
    seed_subject(&harness.core, &b.id, "m3", 1, "hello", 100).await;

    let inbox = harness
        .service
        .list(filter(MailFolder::Inbox, None, None))
        .await
        .unwrap();
    assert_eq!(inbox.len(), 2);
    // internal_date DESC 정렬: m1(300) 먼저.
    assert_eq!(inbox[0].gmail_id, "m1");
    assert_eq!(inbox[0].account_email, "a@x.com");

    let archive = harness
        .service
        .list(filter(MailFolder::Archive, None, None))
        .await
        .unwrap();
    assert_eq!(archive.len(), 1);
    assert_eq!(archive[0].gmail_id, "m2");

    let a_all = harness
        .service
        .list(filter(MailFolder::All, Some(&a.id), None))
        .await
        .unwrap();
    assert_eq!(a_all.len(), 2);

    let matched = harness
        .service
        .list(filter(MailFolder::Inbox, None, Some("invo")))
        .await
        .unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].gmail_id, "m1");
}

async fn seed_unread(
    core: &TodoCore,
    account_id: &str,
    gmail_id: &str,
    in_inbox: i64,
    is_unread: i64,
) {
    sqlx::query(
        "INSERT INTO gmail_messages \
         (account_id, gmail_id, thread_id, internal_date, in_inbox, is_unread, updated_at) \
         VALUES (?, ?, 't1', 0, ?, ?, '2026-07-14T00:00:00.000Z')",
    )
    .bind(account_id)
    .bind(gmail_id)
    .bind(in_inbox)
    .bind(is_unread)
    .execute(core.pool())
    .await
    .expect("seed unread");
}

#[tokio::test]
async fn unread_count_sums_inbox_unread_across_accounts() {
    let harness = harness().await;
    let a = todo_gmail::store::insert_account(harness.core.pool(), "a@x.com")
        .await
        .unwrap();
    let b = todo_gmail::store::insert_account(harness.core.pool(), "b@x.com")
        .await
        .unwrap();
    seed_unread(&harness.core, &a.id, "m1", 1, 1).await; // 받은편지함 안읽음 → 셈
    seed_unread(&harness.core, &a.id, "m2", 1, 0).await; // 읽음 → 제외
    seed_unread(&harness.core, &a.id, "m3", 0, 1).await; // 보관 안읽음 → 제외
    seed_unread(&harness.core, &b.id, "m4", 1, 1).await; // 다른 계정 안읽음 → 셈

    assert_eq!(harness.service.unread_count().await.unwrap(), 2);
}

#[tokio::test]
async fn unread_count_drops_when_message_read_or_archived() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "a@x.com")
        .await
        .unwrap();
    seed_unread(&harness.core, &account.id, "m1", 1, 1).await;
    seed_unread(&harness.core, &account.id, "m2", 1, 1).await;
    assert_eq!(harness.service.unread_count().await.unwrap(), 2);

    harness
        .service
        .set_read(&account.id, "m1", true)
        .await
        .unwrap();
    assert_eq!(harness.service.unread_count().await.unwrap(), 1);

    harness
        .service
        .archive(&account.id, "m2")
        .await
        .unwrap();
    assert_eq!(harness.service.unread_count().await.unwrap(), 0);
}

#[tokio::test]
async fn list_marks_messages_linked_to_active_todos() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "a@x.com")
        .await
        .unwrap();
    seed_subject(&harness.core, &account.id, "m1", 1, "linked mail", 300).await;

    let before = harness
        .service
        .list(filter(MailFolder::All, Some(&account.id), None))
        .await
        .unwrap();
    assert_eq!(before.len(), 1);
    assert!(!before[0].has_todo);

    let todo = harness
        .core
        .create_todo_from_email(
            "linked mail".to_owned(),
            EmailLinkInput {
                account_id: account.id.clone(),
                gmail_id: "m1".to_owned(),
                thread_id: "t1".to_owned(),
                subject: "linked mail".to_owned(),
                from_name: "Kim".to_owned(),
                from_email: "kim@x.com".to_owned(),
            },
        )
        .await
        .expect("create todo from email");

    let after = harness
        .service
        .list(filter(MailFolder::All, Some(&account.id), None))
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].has_todo);

    harness
        .core
        .delete_todo(todo.id)
        .await
        .expect("delete todo");
    let after_delete = harness
        .service
        .list(filter(MailFolder::All, Some(&account.id), None))
        .await
        .unwrap();
    assert_eq!(after_delete.len(), 1);
    assert!(!after_delete[0].has_todo);
}

async fn pending_outbox(core: &TodoCore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM gmail_outbox WHERE completed_at IS NULL")
        .fetch_one(core.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn archive_optimistic_then_pushes_modify() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;
    seed_message(&harness.core, &account.id, "m1", 1, 0).await;

    harness.service.archive(&account.id, "m1").await.unwrap();
    // 낙관적: 로컬은 즉시 inbox 에서 빠졌다.
    assert_eq!(in_inbox_flag(&harness.core, &account.id, "m1").await, 0);
    assert_eq!(pending_outbox(&harness.core).await, 1);

    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/m1/modify"))
        .and(body_string_contains("removeLabelIds"))
        .and(body_string_contains("INBOX"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "m1" })))
        .mount(&harness.mock)
        .await;

    harness.service.process_outbox_once().await.unwrap();
    assert_eq!(pending_outbox(&harness.core).await, 0);
}

#[tokio::test]
async fn remove_account_deletes_token_and_messages() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    seed_message(&harness.core, &account.id, "m1", 1, 0).await;

    harness.service.remove_account(&account.id).await.unwrap();

    assert!(harness.service.accounts().await.unwrap().is_empty());
    assert!(harness.tokens.get("me@x.com").unwrap().is_none());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gmail_messages")
        .fetch_one(harness.core.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn set_read_enqueues_and_pushes() {
    let harness = harness().await;
    let account = todo_gmail::store::insert_account(harness.core.pool(), "me@x.com")
        .await
        .unwrap();
    harness.tokens.set("me@x.com", "rt-1").unwrap();
    mount_token(&harness.mock).await;
    seed_message(&harness.core, &account.id, "m1", 1, 1).await;

    harness
        .service
        .set_read(&account.id, "m1", true)
        .await
        .unwrap();
    // 읽음 처리: is_unread = 0
    let is_unread: i64 = sqlx::query_scalar(
        "SELECT is_unread FROM gmail_messages WHERE account_id = ? AND gmail_id = 'm1'",
    )
    .bind(&account.id)
    .fetch_one(harness.core.pool())
    .await
    .unwrap();
    assert_eq!(is_unread, 0);

    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/m1/modify"))
        .and(body_string_contains("removeLabelIds"))
        .and(body_string_contains("UNREAD"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "m1" })))
        .mount(&harness.mock)
        .await;

    harness.service.process_outbox_once().await.unwrap();
    assert_eq!(pending_outbox(&harness.core).await, 0);
}

#[tokio::test]
async fn migration_creates_gmail_tables() {
    let (_db, core) = connect().await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
         ('gmail_accounts','gmail_messages','gmail_outbox')",
    )
    .fetch_one(core.pool())
    .await
    .expect("query");
    assert_eq!(count, 3);

    let store = MemoryTokenStore::default();
    store.set("a@x.com", "refresh-1").unwrap();
    assert_eq!(store.get("a@x.com").unwrap().as_deref(), Some("refresh-1"));
    let _ = Arc::new(store);
}

#[tokio::test]
async fn accounts_insert_list_delete_roundtrip() {
    let (_db, core) = connect().await;
    let a = todo_gmail::store::insert_account(core.pool(), "a@x.com")
        .await
        .unwrap();
    let b = todo_gmail::store::insert_account(core.pool(), "b@x.com")
        .await
        .unwrap();
    assert_ne!(a.color, b.color);

    // 같은 이메일 재삽입은 기존 계정을 반환한다.
    let a_again = todo_gmail::store::insert_account(core.pool(), "a@x.com")
        .await
        .unwrap();
    assert_eq!(a.id, a_again.id);

    let all = todo_gmail::store::list_accounts(core.pool()).await.unwrap();
    assert_eq!(all.len(), 2);

    todo_gmail::store::delete_account(core.pool(), &a.id)
        .await
        .unwrap();
    let remaining = todo_gmail::store::list_accounts(core.pool()).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, b.id);
}

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
    assert!(url.contains("access_type=offline"));
}
