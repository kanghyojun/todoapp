use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tempfile::NamedTempFile;
use todo_core::TodoCore;
use todo_gmail::{TokenStore, TokenStoreError};

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
