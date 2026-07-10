use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use todo_core::{CreateTodoInput, Error as CoreError, LinearLinkInput, Priority, Status, TodoCore};
use todo_linear::{Error, KeyStore, KeyStoreError, LinearService};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, header, method},
};

const API_KEY: &str = "linear-test-key";

#[derive(Default)]
struct MemoryKeyStore {
    value: Mutex<Option<String>>,
}

impl MemoryKeyStore {
    fn configured() -> Self {
        Self {
            value: Mutex::new(Some(API_KEY.to_owned())),
        }
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self) -> Result<Option<String>, KeyStoreError> {
        self.value
            .lock()
            .map_err(|_| KeyStoreError)
            .map(|v| v.clone())
    }

    fn set(&self, value: &str) -> Result<(), KeyStoreError> {
        *self.value.lock().map_err(|_| KeyStoreError)? = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        *self.value.lock().map_err(|_| KeyStoreError)? = None;
        Ok(())
    }
}

struct Harness {
    _database: NamedTempFile,
    core: TodoCore,
    mock: MockServer,
    service: LinearService,
}

impl Harness {
    async fn configured() -> Self {
        Self::new(Arc::new(MemoryKeyStore::configured())).await
    }

    async fn new(keys: Arc<dyn KeyStore>) -> Self {
        let database = NamedTempFile::new().expect("create database");
        let core = TodoCore::connect(&format!("sqlite://{}", database.path().display()))
            .await
            .expect("connect core");
        sqlx::query("INSERT INTO settings (key, value) VALUES ('linear.viewer_id', 'viewer-1')")
            .execute(core.pool())
            .await
            .expect("set viewer id");
        let mock = MockServer::start().await;
        let service =
            LinearService::with_endpoint(core.clone(), keys, format!("{}/graphql", mock.uri()));
        Self {
            _database: database,
            core,
            mock,
            service,
        }
    }

    async fn mount_graphql(&self, operation: &str, response: Value) {
        Mock::given(method("POST"))
            .and(header("authorization", API_KEY))
            .and(body_string_contains(operation))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&self.mock)
            .await;
    }

    async fn create_linked(&self, issue_id: &str, team_id: &str) -> todo_core::Todo {
        let todo = self
            .core
            .create_todo(CreateTodoInput::new(format!("todo {issue_id}")))
            .await
            .expect("create todo");
        self.core
            .link_linear(todo.id, link_input(issue_id, team_id))
            .await
            .expect("link todo");
        todo
    }
}

#[tokio::test]
async fn link_resolves_identifier_and_url_without_changing_local_text() {
    let harness = Harness::configured().await;
    harness
        .mount_graphql("ResolveIssue", json!({ "data": { "issue": issue("issue-1", "PI-1234", "remote", 2, "started", "team-1") } }))
        .await;
    let first = harness
        .core
        .create_todo(CreateTodoInput {
            title: "local title".to_owned(),
            description: "local description".to_owned(),
            ..CreateTodoInput::default()
        })
        .await
        .expect("create first todo");
    harness
        .service
        .link(first.id, "PI-1234")
        .await
        .expect("link identifier");
    let unchanged = harness.core.get_todo(first.id).await.expect("read todo");
    assert_eq!(unchanged.title, "local title");
    assert_eq!(unchanged.description, "local description");

    let url_harness = Harness::configured().await;
    url_harness
        .mount_graphql("ResolveIssue", json!({ "data": { "issue": issue("issue-2", "PI-5678", "other", 0, "started", "team-1") } }))
        .await;
    let second = url_harness
        .core
        .create_todo(CreateTodoInput::new("URL todo"))
        .await
        .expect("create second todo");
    url_harness
        .service
        .link(
            second.id,
            "https://linear.app/acme/issue/PI-5678/a-descriptive-slug",
        )
        .await
        .expect("link URL");
    let identifiers =
        sqlx::query_scalar::<_, String>("SELECT identifier FROM linear_links ORDER BY identifier")
            .fetch_all(url_harness.core.pool())
            .await
            .expect("read links");
    assert_eq!(identifiers, ["PI-5678"]);
}

#[tokio::test]
async fn duplicate_resolved_issue_surfaces_issue_already_linked() {
    let harness = Harness::configured().await;
    harness
        .mount_graphql(
            "ResolveIssue",
            json!({ "data": { "issue": issue("same", "PI-1", "remote", 0, "started", "team-1") } }),
        )
        .await;
    let first = harness
        .core
        .create_todo(CreateTodoInput::new("first"))
        .await
        .expect("create first");
    let second = harness
        .core
        .create_todo(CreateTodoInput::new("second"))
        .await
        .expect("create second");
    harness
        .service
        .link(first.id, "PI-1")
        .await
        .expect("link first");
    let error = harness
        .service
        .link(second.id, "PI-1")
        .await
        .expect_err("duplicate link must fail");
    assert!(matches!(error, Error::Core(CoreError::IssueAlreadyLinked)));
}

#[tokio::test]
async fn missing_resolved_issue_is_an_error() {
    let harness = Harness::configured().await;
    harness
        .mount_graphql("ResolveIssue", json!({ "data": { "issue": null } }))
        .await;
    let todo = harness
        .core
        .create_todo(CreateTodoInput::new("missing issue"))
        .await
        .expect("create todo");
    let error = harness
        .service
        .link(todo.id, "PI-404")
        .await
        .expect_err("missing issue must fail");
    assert!(matches!(error, Error::IssueNotFound));
}

#[tokio::test]
async fn pull_creates_unlinked_issue_with_linear_fields() {
    let harness = Harness::configured().await;
    harness
        .mount_graphql("PullInProgress", json!({ "data": { "issues": { "nodes": [issue("new-1", "PI-10", "Linear title", 2, "started", "team-1")] } } }))
        .await;
    harness
        .mount_graphql("ReverseStates", json!({ "data": { "issues": { "nodes": [issue("new-1", "PI-10", "Linear title", 2, "started", "team-1")] } } }))
        .await;
    let result = harness.service.pull().await.expect("pull issues");
    assert_eq!(result.created, 1);
    assert_eq!(result.skipped, 0);
    let todos = harness
        .core
        .list_todos(Default::default())
        .await
        .expect("list todos");
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0].title, "Linear title");
    assert_eq!(todos[0].description, "description new-1");
    assert_eq!(todos[0].status, Status::InProgress);
    assert_eq!(todos[0].priority, Priority::High);
}

#[tokio::test]
async fn pull_skips_existing_and_soft_deleted_links_without_refresh_or_resurrection() {
    let harness = Harness::configured().await;
    let existing = harness.create_linked("existing", "team-1").await;
    let deleted = harness.create_linked("deleted", "team-1").await;
    harness
        .core
        .delete_todo(deleted.id)
        .await
        .expect("soft delete linked todo");
    harness
        .mount_graphql(
            "PullInProgress",
            json!({ "data": { "issues": { "nodes": [
            issue("existing", "PI-20", "changed remotely", 1, "started", "team-1"),
            issue("deleted", "PI-21", "would resurrect", 1, "started", "team-1")
        ] } } }),
        )
        .await;
    harness
        .mount_graphql("ReverseStates", json!({ "data": { "issues": { "nodes": [issue("existing", "PI-20", "changed remotely", 1, "started", "team-1")] } } }))
        .await;

    let result = harness.service.pull().await.expect("pull existing issues");
    assert_eq!(result.created, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(
        harness
            .core
            .get_todo(existing.id)
            .await
            .expect("read existing")
            .title,
        "todo existing"
    );
    assert!(harness.core.get_todo(deleted.id).await.is_err());
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM todos")
        .fetch_one(harness.core.pool())
        .await
        .expect("count todos");
    assert_eq!(total, 2);
}

#[tokio::test]
async fn pull_closes_completed_and_canceled_without_outbox_rows() {
    let harness = Harness::configured().await;
    let completed = harness.create_linked("completed", "team-1").await;
    let canceled = harness.create_linked("canceled", "team-1").await;
    harness
        .mount_graphql(
            "PullInProgress",
            json!({ "data": { "issues": { "nodes": [] } } }),
        )
        .await;
    harness
        .mount_graphql(
            "ReverseStates",
            json!({ "data": { "issues": { "nodes": [
            issue("completed", "PI-30", "done", 0, "completed", "team-1"),
            issue("canceled", "PI-31", "canceled", 0, "canceled", "team-1")
        ] } } }),
        )
        .await;

    let result = harness
        .service
        .pull()
        .await
        .expect("pull remote completion");
    assert_eq!(result.closed_locally, 2);
    assert_eq!(
        harness
            .core
            .get_todo(completed.id)
            .await
            .expect("completed todo")
            .status,
        Status::Done
    );
    assert_eq!(
        harness
            .core
            .get_todo(canceled.id)
            .await
            .expect("canceled todo")
            .status,
        Status::Done
    );
    let outbox = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(harness.core.pool())
        .await
        .expect("count outbox");
    assert_eq!(outbox, 0);
}

#[tokio::test]
async fn worker_resolves_single_done_state_pushes_and_updates_link() {
    let harness = Harness::configured().await;
    let todo = harness.create_linked("push-1", "team-1").await;
    harness
        .core
        .set_status(todo.id, Status::Done)
        .await
        .expect("enqueue completion");
    harness
        .mount_graphql("CompletedStates", json!({ "data": { "workflowStates": { "nodes": [{ "id": "done-state", "name": "Done" }] } } }))
        .await;
    harness
        .mount_graphql(
            "CompleteIssue",
            json!({ "data": { "issueUpdate": { "success": true } } }),
        )
        .await;

    let result = harness
        .service
        .process_outbox_once()
        .await
        .expect("process outbox");
    assert_eq!(result.pushed, 1);
    let completed_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT completed_at FROM sync_outbox WHERE todo_id = ?",
    )
    .bind(todo.id.to_string())
    .fetch_one(harness.core.pool())
    .await
    .expect("read outbox completion");
    assert!(completed_at.is_some());
    let last_status = sqlx::query_scalar::<_, Option<String>>(
        "SELECT last_pushed_status FROM linear_links WHERE todo_id = ?",
    )
    .bind(todo.id.to_string())
    .fetch_one(harness.core.pool())
    .await
    .expect("read pushed status");
    assert_eq!(last_status.as_deref(), Some("done"));
}

#[tokio::test]
async fn multiple_done_states_wait_for_choice_then_push() {
    let harness = Harness::configured().await;
    let todo = harness.create_linked("choice-1", "team-choice").await;
    harness
        .core
        .set_status(todo.id, Status::Done)
        .await
        .expect("enqueue completion");
    harness
        .mount_graphql(
            "CompletedStates",
            json!({ "data": { "workflowStates": { "nodes": [
            { "id": "done", "name": "Done" }, { "id": "merged", "name": "Merged" }
        ] } } }),
        )
        .await;
    let first = harness
        .service
        .process_outbox_once()
        .await
        .expect("discover choices");
    assert_eq!(first.needs_choice, 1);
    let choices = harness
        .service
        .pending_choices()
        .await
        .expect("list choices");
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].candidate_states.len(), 2);
    let pending =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox WHERE completed_at IS NULL")
            .fetch_one(harness.core.pool())
            .await
            .expect("count pending");
    assert_eq!(pending, 1);

    harness
        .service
        .set_done_state("team-choice", "merged")
        .await
        .expect("choose done state");
    harness
        .mount_graphql(
            "CompleteIssue",
            json!({ "data": { "issueUpdate": { "success": true } } }),
        )
        .await;
    let second = harness
        .service
        .process_outbox_once()
        .await
        .expect("push after choice");
    assert_eq!(second.pushed, 1);
}

#[tokio::test]
async fn server_failures_follow_the_required_backoff_sequence() {
    let harness = Harness::configured().await;
    let todo = harness.create_linked("failure-1", "team-1").await;
    harness
        .core
        .set_status(todo.id, Status::Done)
        .await
        .expect("enqueue completion");
    sqlx::query("INSERT INTO settings (key, value) VALUES ('linear.done_state.team-1', 'done')")
        .execute(harness.core.pool())
        .await
        .expect("set done state");
    Mock::given(method("POST"))
        .and(body_string_contains("CompleteIssue"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&harness.mock)
        .await;

    for (index, expected_seconds) in [5_i64, 15, 60, 300, 1_800].into_iter().enumerate() {
        let before = Utc::now();
        let result = harness
            .service
            .process_outbox_once()
            .await
            .expect("record failure");
        assert_eq!(result.failed, 1);
        let (attempts, next): (i64, String) =
            sqlx::query_as("SELECT attempts, next_attempt_at FROM sync_outbox WHERE todo_id = ?")
                .bind(todo.id.to_string())
                .fetch_one(harness.core.pool())
                .await
                .expect("read retry state");
        assert_eq!(attempts, index as i64 + 1);
        let next = DateTime::parse_from_rfc3339(&next)
            .expect("parse next attempt")
            .with_timezone(&Utc);
        let actual = (next - before).num_seconds();
        assert!(
            (expected_seconds - 1..=expected_seconds + 1).contains(&actual),
            "expected {expected_seconds}s, got {actual}s"
        );
        sqlx::query("UPDATE sync_outbox SET next_attempt_at = ? WHERE todo_id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(todo.id.to_string())
            .execute(harness.core.pool())
            .await
            .expect("make retry ready");
    }
}

#[tokio::test]
async fn http_429_and_graphql_rate_limit_errors_are_retryable() {
    for response in [
        ResponseTemplate::new(429),
        ResponseTemplate::new(400).set_body_json(json!({
            "errors": [{ "extensions": { "code": "RATELIMITED" } }]
        })),
    ] {
        let harness = Harness::configured().await;
        let todo = harness.create_linked("rate-limited", "team-1").await;
        harness
            .core
            .set_status(todo.id, Status::Done)
            .await
            .expect("enqueue completion");
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('linear.done_state.team-1', 'done')",
        )
        .execute(harness.core.pool())
        .await
        .expect("set done state");
        Mock::given(method("POST"))
            .and(body_string_contains("CompleteIssue"))
            .respond_with(response)
            .mount(&harness.mock)
            .await;

        harness
            .service
            .process_outbox_once()
            .await
            .expect("record rate limit");
        let (attempts, next): (i64, String) =
            sqlx::query_as("SELECT attempts, next_attempt_at FROM sync_outbox WHERE todo_id = ?")
                .bind(todo.id.to_string())
                .fetch_one(harness.core.pool())
                .await
                .expect("read retry state");
        assert_eq!(attempts, 1);
        let next = DateTime::parse_from_rfc3339(&next).expect("parse retry time");
        assert!(next > Utc::now());
    }
}

#[tokio::test]
async fn unauthorized_is_immediately_failing_and_not_scheduled_for_retry() {
    let harness = Harness::configured().await;
    let todo = harness.create_linked("unauthorized-1", "team-1").await;
    harness
        .core
        .set_status(todo.id, Status::Done)
        .await
        .expect("enqueue completion");
    sqlx::query("INSERT INTO settings (key, value) VALUES ('linear.done_state.team-1', 'done')")
        .execute(harness.core.pool())
        .await
        .expect("set done state");
    Mock::given(method("POST"))
        .and(body_string_contains("CompleteIssue"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&harness.mock)
        .await;

    harness
        .service
        .process_outbox_once()
        .await
        .expect("record unauthorized");
    let (attempts, next): (i64, String) =
        sqlx::query_as("SELECT attempts, next_attempt_at FROM sync_outbox WHERE todo_id = ?")
            .bind(todo.id.to_string())
            .fetch_one(harness.core.pool())
            .await
            .expect("read unauthorized state");
    assert_eq!(attempts, 4);
    assert!(next.starts_with("9999-"));
    let status = harness.service.status().await.expect("read status");
    assert_eq!(status.failing, 1);
}

#[tokio::test]
async fn graphql_errors_in_http_200_are_failures() {
    let harness = Harness::configured().await;
    harness
        .mount_graphql(
            "ResolveIssue",
            json!({ "data": { "issue": null }, "errors": [{ "message": "failure" }] }),
        )
        .await;
    let todo = harness
        .core
        .create_todo(CreateTodoInput::new("GraphQL error"))
        .await
        .expect("create todo");
    let error = harness
        .service
        .link(todo.id, "PI-99")
        .await
        .expect_err("GraphQL errors must fail");
    assert!(matches!(error, Error::Remote { .. }));
}

#[tokio::test]
async fn no_key_blocks_operations_and_worker_does_nothing() {
    let harness = Harness::new(Arc::new(MemoryKeyStore::default())).await;
    let todo = harness.create_linked("no-key", "team-1").await;
    harness
        .core
        .set_status(todo.id, Status::Done)
        .await
        .expect("enqueue completion");
    assert!(matches!(
        harness.service.link(todo.id, "PI-1").await,
        Err(Error::NotConfigured)
    ));
    assert!(matches!(
        harness.service.pull().await,
        Err(Error::NotConfigured)
    ));
    assert!(matches!(
        harness.service.pending_choices().await,
        Err(Error::NotConfigured)
    ));
    assert!(matches!(
        harness.service.retry_failing().await,
        Err(Error::NotConfigured)
    ));
    let worker = harness
        .service
        .process_outbox_once()
        .await
        .expect("worker must idle");
    assert_eq!(worker, Default::default());
    let attempts =
        sqlx::query_scalar::<_, i64>("SELECT attempts FROM sync_outbox WHERE todo_id = ?")
            .bind(todo.id.to_string())
            .fetch_one(harness.core.pool())
            .await
            .expect("read attempts");
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn setting_and_deleting_key_validates_viewer_and_never_persists_secret() {
    let keys = Arc::new(MemoryKeyStore::default());
    let harness = Harness::new(keys.clone()).await;
    harness
        .mount_graphql(
            "Viewer",
            json!({ "data": { "viewer": { "id": "viewer-new" } } }),
        )
        .await;
    harness
        .service
        .set_api_key(API_KEY)
        .await
        .expect("set API key");
    assert!(harness.service.is_configured());
    let settings = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
        .fetch_all(harness.core.pool())
        .await
        .expect("read settings");
    assert!(settings.iter().all(|(_, value)| value != API_KEY));
    assert!(
        settings
            .iter()
            .any(|(key, value)| { key == "linear.viewer_id" && value == "viewer-new" })
    );
    harness
        .service
        .delete_api_key()
        .await
        .expect("delete API key");
    assert!(!harness.service.is_configured());
}

fn issue(
    id: &str,
    identifier: &str,
    title: &str,
    priority: i64,
    state_type: &str,
    team_id: &str,
) -> Value {
    json!({
        "id": id,
        "identifier": identifier,
        "title": title,
        "description": format!("description {id}"),
        "url": format!("https://linear.app/acme/issue/{identifier}/slug"),
        "priority": priority,
        "state": { "id": format!("state-{state_type}"), "name": state_type, "type": state_type },
        "team": { "id": team_id }
    })
}

fn link_input(issue_id: &str, team_id: &str) -> LinearLinkInput {
    LinearLinkInput {
        issue_id: issue_id.to_owned(),
        identifier: format!("PI-{issue_id}"),
        url: format!("https://linear.app/acme/issue/PI-{issue_id}/slug"),
        team_id: team_id.to_owned(),
    }
}
