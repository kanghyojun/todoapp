use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tempfile::NamedTempFile;
use todo_core::TodoCore;
use todo_gmail::{GmailService, TokenStore, TokenStoreError};
use wiremock::matchers::{method, path};
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
    assert_eq!(account.history_id.as_deref(), Some("12345"));
    assert_eq!(
        harness.tokens.get("me@x.com").unwrap().as_deref(),
        Some("rt-1")
    );
    let accounts = todo_gmail::store::list_accounts(harness.core.pool())
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
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
