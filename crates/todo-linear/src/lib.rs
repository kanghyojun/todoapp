use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{SecondsFormat, TimeDelta, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{FromRow, Sqlite, Transaction};
use thiserror::Error;
use todo_core::{
    CreateTodoInput, Error as CoreError, LinearLinkInput, Priority, Status, TodoCore, TodoId,
};
use tokio::sync::RwLock;

const DEFAULT_ENDPOINT: &str = "https://api.linear.app/graphql";
const KEYRING_SERVICE: &str = "todo";
const KEYRING_USER: &str = "linear-api-key";

pub trait KeyStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, KeyStoreError>;
    fn set(&self, value: &str) -> Result<(), KeyStoreError>;
    fn delete(&self) -> Result<(), KeyStoreError>;
}

#[derive(Debug, Error)]
#[error("key store operation failed")]
pub struct KeyStoreError;

#[derive(Debug, Default)]
pub struct SystemKeyStore;

impl KeyStore for SystemKeyStore {
    fn get(&self) -> Result<Option<String>, KeyStoreError> {
        match keyring_entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(KeyStoreError),
        }
    }

    fn set(&self, value: &str) -> Result<(), KeyStoreError> {
        keyring_entry()?
            .set_password(value)
            .map_err(|_| KeyStoreError)
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        match keyring_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(KeyStoreError),
        }
    }
}

fn keyring_entry() -> Result<keyring::Entry, KeyStoreError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|_| KeyStoreError)
}

#[derive(Clone)]
pub struct LinearService {
    core: TodoCore,
    client: reqwest::Client,
    endpoint: String,
    keys: Arc<dyn KeyStore>,
    // 키체인은 서명 안 된 개발 빌드에서 읽을 때마다 프롬프트를 띄운다. 프로세스
    // 수명 동안 한 번만 읽고 메모리에 둔다. 바깥 Option = 아직 안 읽음, 안쪽
    // Option = 키 값(None 은 키 없음). set/delete 가 이 값을 갱신한다.
    key_cache: Arc<Mutex<Option<Option<String>>>>,
    needs_choice: Arc<RwLock<HashMap<String, Vec<WorkflowState>>>>,
}

impl LinearService {
    pub fn new(core: TodoCore, keys: Arc<dyn KeyStore>) -> Self {
        Self::with_endpoint(core, keys, DEFAULT_ENDPOINT)
    }

    pub fn with_endpoint(
        core: TodoCore,
        keys: Arc<dyn KeyStore>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            core,
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
            keys,
            key_cache: Arc::new(Mutex::new(None)),
            needs_choice: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn set_api_key(&self, api_key: &str) -> Result<(), Error> {
        if api_key.trim().is_empty() {
            return Err(Error::InvalidInput("api_key must not be blank".to_owned()));
        }
        let viewer: ViewerData = self.graphql(api_key, VIEWER_QUERY, json!({})).await?;
        self.keys.set(api_key)?;
        self.store_key_cache(Some(api_key.to_owned()));
        if let Err(error) = set_setting(&self.core, "linear.viewer_id", &viewer.viewer.id).await {
            let _ = self.keys.delete();
            self.store_key_cache(None);
            return Err(error);
        }
        Ok(())
    }

    pub async fn delete_api_key(&self) -> Result<(), Error> {
        self.keys.delete()?;
        self.store_key_cache(None);
        sqlx::query("DELETE FROM settings WHERE key = 'linear.viewer_id'")
            .execute(self.core.pool())
            .await?;
        self.needs_choice.write().await.clear();
        Ok(())
    }

    /// 키체인을 프로세스당 한 번만 읽고 캐시한다. 성공(Ok)만 캐시한다. 실패는
    /// 캐시하지 않아 잠금이 풀리면 다음 호출에서 다시 시도한다.
    fn cached_key(&self) -> Result<Option<String>, KeyStoreError> {
        if let Some(cached) = self.key_cache.lock().unwrap().as_ref() {
            return Ok(cached.clone());
        }
        let value = self.keys.get()?;
        *self.key_cache.lock().unwrap() = Some(value.clone());
        Ok(value)
    }

    /// 키를 넣거나 지운 뒤 캐시를 그 값으로 맞춘다.
    fn store_key_cache(&self, value: Option<String>) {
        *self.key_cache.lock().unwrap() = Some(value);
    }

    /// 키를 읽을 수 없는 상황은 키가 없는 것과 같이 다룬다.
    /// 키체인이 없는 기기(헤드리스 리눅스)나 잠긴 키체인에서 조회가 실패하는데,
    /// 그걸 500 으로 올리면 "Linear 를 쓰고 있냐"는 질문조차 답할 수 없다.
    fn read_key(&self) -> Option<String> {
        self.cached_key().ok().flatten()
    }

    pub fn is_configured(&self) -> bool {
        self.read_key().is_some()
    }

    /// 키체인 자체를 열 수 있는지. `is_configured` 가 false 일 때 이유를 가른다.
    pub fn key_store_available(&self) -> bool {
        self.cached_key().is_ok()
    }

    pub async fn link(&self, todo_id: TodoId, issue_ref: &str) -> Result<(), Error> {
        let key = self.require_key()?;
        self.core.get_todo(todo_id).await?;
        let identifier = issue_identifier(issue_ref)?;
        let data: IssueData = self
            .graphql(&key, ISSUE_QUERY, json!({ "issueRef": identifier }))
            .await?;
        let issue = data.issue.ok_or(Error::IssueNotFound)?;
        self.core
            .link_linear(todo_id, issue.as_link_input())
            .await?;
        Ok(())
    }

    pub async fn pull(&self) -> Result<PullSummary, Error> {
        let key = self.require_key()?;
        let viewer_id = get_setting(&self.core, "linear.viewer_id")
            .await?
            .ok_or(Error::NotConfigured)?;
        let data: IssuesData = self
            .graphql(&key, PULL_QUERY, json!({ "viewerId": viewer_id }))
            .await?;
        let linked_issue_ids = sqlx::query_scalar::<_, String>("SELECT issue_id FROM linear_links")
            .fetch_all(self.core.pool())
            .await?;
        let mut linked: std::collections::HashSet<String> = linked_issue_ids.into_iter().collect();
        let mut summary = PullSummary::default();

        for issue in data.issues.nodes {
            if linked.contains(&issue.id) {
                summary.skipped += 1;
                continue;
            }
            let todo = self
                .core
                .create_todo(CreateTodoInput {
                    title: issue.title.clone(),
                    description: issue.description.clone().unwrap_or_default(),
                    status: Status::InProgress,
                    priority: priority_from_linear(issue.priority)?,
                    due_date: None,
                })
                .await?;
            self.core
                .link_linear(todo.id, issue.as_link_input())
                .await?;
            linked.insert(issue.id);
            summary.created += 1;
        }

        summary.closed_locally = self.close_remote_completed(&key).await?;
        Ok(summary)
    }

    pub async fn process_outbox_once(&self) -> Result<WorkerSummary, Error> {
        // 일감을 먼저 확인한다. 아웃박스가 비면 키체인을 아예 열지 않는다.
        // 이 워커가 5초마다 돌면서 키를 먼저 읽던 것이, 서명 안 된 개발 빌드에서
        // 유휴 상태에도 키체인 프롬프트가 쏟아지던 원인이었다.
        let rows = sqlx::query_as::<_, OutboxRow>(
            "SELECT o.id, o.attempts, l.issue_id, l.team_id, t.status \
             FROM sync_outbox o \
             JOIN linear_links l ON l.todo_id = o.todo_id \
             JOIN todos t ON t.id = o.todo_id \
             WHERE o.completed_at IS NULL AND o.next_attempt_at <= ? \
             ORDER BY o.created_at ASC, o.id ASC",
        )
        .bind(now_string())
        .fetch_all(self.core.pool())
        .await?;
        if rows.is_empty() {
            return Ok(WorkerSummary::default());
        }
        let Some(key) = self.cached_key()? else {
            return Ok(WorkerSummary::default());
        };
        let mut summary = WorkerSummary::default();
        for row in rows {
            match self.process_outbox_row(&key, &row).await {
                Ok(ProcessOutcome::Pushed) => summary.pushed += 1,
                Ok(ProcessOutcome::NeedsChoice) => summary.needs_choice += 1,
                Ok(ProcessOutcome::Skipped) => summary.skipped += 1,
                Err(error) => {
                    self.record_failure(&row, &error).await?;
                    summary.failed += 1;
                }
            }
        }
        Ok(summary)
    }

    pub async fn pending_choices(&self) -> Result<Vec<PendingChoice>, Error> {
        self.require_key()?;
        let choices = self.needs_choice.read().await;
        let mut values = choices
            .iter()
            .map(|(team_id, states)| PendingChoice {
                team_id: team_id.clone(),
                candidate_states: states.clone(),
            })
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.team_id.cmp(&right.team_id));
        Ok(values)
    }

    pub async fn set_done_state(&self, team_id: &str, state_id: &str) -> Result<(), Error> {
        self.require_key()?;
        if team_id.trim().is_empty() || state_id.trim().is_empty() {
            return Err(Error::InvalidInput(
                "team_id and state_id must not be blank".to_owned(),
            ));
        }
        set_setting(
            &self.core,
            &format!("linear.done_state.{team_id}"),
            state_id,
        )
        .await?;
        self.needs_choice.write().await.remove(team_id);
        Ok(())
    }

    /// 키가 없어도, 키체인을 못 열어도 성공한다. UI 가 Linear 배지를 그리려면
    /// 이 질문에는 언제나 답할 수 있어야 한다.
    pub async fn status(&self) -> Result<LinearStatus, Error> {
        let configured = self.is_configured();
        let row = sqlx::query_as::<_, CountRow>(
            "SELECT COUNT(*) AS pending, \
                        COALESCE(SUM(CASE WHEN attempts > 3 THEN 1 ELSE 0 END), 0) AS failing \
                 FROM sync_outbox WHERE completed_at IS NULL",
        )
        .fetch_one(self.core.pool())
        .await?;
        let needs_choice = if configured {
            self.pending_choices().await?
        } else {
            Vec::new()
        };
        Ok(LinearStatus {
            configured,
            key_store_available: self.key_store_available(),
            pending: row.pending,
            failing: row.failing,
            needs_choice,
        })
    }

    pub async fn retry_failing(&self) -> Result<u64, Error> {
        self.require_key()?;
        Ok(sqlx::query(
            "UPDATE sync_outbox SET next_attempt_at = ? \
             WHERE completed_at IS NULL AND attempts > 3",
        )
        .bind(now_string())
        .execute(self.core.pool())
        .await?
        .rows_affected())
    }

    pub fn spawn_worker(&self) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(error) = service.process_outbox_once().await {
                    eprintln!("todo-linear worker error: {error}");
                }
            }
        })
    }

    async fn close_remote_completed(&self, key: &str) -> Result<u64, Error> {
        let rows = sqlx::query_as::<_, LinkedActiveTodo>(
            "SELECT t.id AS todo_id, l.issue_id \
             FROM todos t JOIN linear_links l ON l.todo_id = t.id \
             WHERE t.deleted_at IS NULL AND t.status != 'done'",
        )
        .fetch_all(self.core.pool())
        .await?;
        if rows.is_empty() {
            return Ok(0);
        }
        let issue_ids = rows
            .iter()
            .map(|row| row.issue_id.clone())
            .collect::<Vec<_>>();
        let data: IssuesData = self
            .graphql(key, REVERSE_STATES_QUERY, json!({ "issueIds": issue_ids }))
            .await?;
        let remote_states = data
            .issues
            .nodes
            .into_iter()
            .map(|issue| (issue.id, issue.state.kind))
            .collect::<HashMap<_, _>>();
        let mut closed = 0;
        for row in rows {
            if remote_states
                .get(&row.issue_id)
                .is_some_and(|state| state == "completed" || state == "canceled")
            {
                let todo_id = TodoId::from_str(&row.todo_id)
                    .map_err(|_| Error::Database("invalid todo id in linear link".to_owned()))?;
                self.core.mark_done_from_remote(todo_id).await?;
                closed += 1;
            }
        }
        Ok(closed)
    }

    async fn process_outbox_row(
        &self,
        key: &str,
        row: &OutboxRow,
    ) -> Result<ProcessOutcome, Error> {
        // 완료 명령을 큐에 넣은 뒤 사용자가 완료를 되돌렸을 수 있다. 취소를
        // 놓친 레이스까지 대비해, 더 이상 완료가 아닌 항목은 이슈를 닫지 않고
        // 이 행을 정리만 한다. 재시도로 다시 밀리지 않게 완료 처리한다.
        if row.status != "done" {
            sqlx::query("UPDATE sync_outbox SET completed_at = ?, last_error = NULL WHERE id = ?")
                .bind(now_string())
                .bind(row.id)
                .execute(self.core.pool())
                .await?;
            return Ok(ProcessOutcome::Skipped);
        }
        let setting_key = format!("linear.done_state.{}", row.team_id);
        let configured_state = get_setting(&self.core, &setting_key).await?;
        if configured_state.is_none() && self.needs_choice.read().await.contains_key(&row.team_id) {
            return Ok(ProcessOutcome::NeedsChoice);
        }
        let state_id = if let Some(state_id) = configured_state {
            state_id
        } else {
            let data: WorkflowStatesData = self
                .graphql(key, WORKFLOW_STATES_QUERY, json!({ "teamId": row.team_id }))
                .await?;
            match data.workflow_states.nodes.as_slice() {
                [state] => {
                    set_setting(&self.core, &setting_key, &state.id).await?;
                    state.id.clone()
                }
                states if states.len() > 1 => {
                    self.needs_choice
                        .write()
                        .await
                        .insert(row.team_id.clone(), states.to_vec());
                    return Ok(ProcessOutcome::NeedsChoice);
                }
                _ => {
                    return Err(Error::Remote {
                        message: "Linear team has no completed workflow state".to_owned(),
                        retryable: true,
                    });
                }
            }
        };
        let data: IssueUpdateData = self
            .graphql(
                key,
                ISSUE_UPDATE_MUTATION,
                json!({ "issueId": row.issue_id, "stateId": state_id }),
            )
            .await?;
        if !data.issue_update.success {
            return Err(Error::Remote {
                message: "Linear rejected the issue update".to_owned(),
                retryable: true,
            });
        }
        let mut transaction = self.core.pool().begin().await?;
        complete_outbox_row(&mut transaction, row.id, &row.issue_id).await?;
        transaction.commit().await?;
        self.needs_choice.write().await.remove(&row.team_id);
        Ok(ProcessOutcome::Pushed)
    }

    async fn record_failure(&self, row: &OutboxRow, error: &Error) -> Result<(), Error> {
        let attempts = if error.is_retryable() {
            row.attempts + 1
        } else {
            (row.attempts + 1).max(4)
        };
        let next_attempt_at = if error.is_retryable() {
            let delay = backoff_for_attempt(attempts);
            (Utc::now() + delay).to_rfc3339_opts(SecondsFormat::Millis, true)
        } else {
            "9999-12-31T23:59:59.999Z".to_owned()
        };
        sqlx::query(
            "UPDATE sync_outbox SET attempts = ?, next_attempt_at = ?, last_error = ? \
             WHERE id = ? AND completed_at IS NULL",
        )
        .bind(attempts)
        .bind(next_attempt_at)
        .bind(error.safe_message())
        .bind(row.id)
        .execute(self.core.pool())
        .await?;
        Ok(())
    }

    fn require_key(&self) -> Result<String, Error> {
        self.read_key().ok_or(Error::NotConfigured)
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        key: &str,
        query: &str,
        variables: Value,
    ) -> Result<T, Error> {
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, key)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|_| Error::Remote {
                message: "could not reach Linear".to_owned(),
                retryable: true,
            })?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let rate_limited = if status == StatusCode::TOO_MANY_REQUESTS {
                true
            } else if status == StatusCode::BAD_REQUEST {
                response
                    .json::<GraphqlErrors>()
                    .await
                    .is_ok_and(|body| body.errors.iter().any(GraphqlError::is_rate_limited))
            } else {
                false
            };
            return Err(Error::Remote {
                message: format!("Linear returned HTTP {status}"),
                retryable: rate_limited || status.is_server_error(),
            });
        }
        let envelope: GraphqlEnvelope<T> = response.json().await.map_err(|_| Error::Remote {
            message: "Linear returned an invalid GraphQL response".to_owned(),
            retryable: true,
        })?;
        if !envelope.errors.is_empty() {
            return Err(Error::Remote {
                message: "Linear GraphQL returned errors".to_owned(),
                retryable: true,
            });
        }
        envelope.data.ok_or_else(|| Error::Remote {
            message: "Linear GraphQL response had no data".to_owned(),
            retryable: true,
        })
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Linear is not configured")]
    NotConfigured,
    #[error("Linear rejected the API key")]
    Unauthorized,
    #[error("Linear issue was not found")]
    IssueNotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{message}")]
    Remote { message: String, retryable: bool },
    #[error("database error: {0}")]
    Database(String),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    KeyStore(#[from] KeyStoreError),
}

impl Error {
    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Remote {
                retryable: true,
                ..
            }
        )
    }

    fn safe_message(&self) -> String {
        match self {
            Self::Remote { message, .. } => message.clone(),
            Self::Unauthorized => "Linear rejected the API key".to_owned(),
            _ => self.to_string(),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.to_string())
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PullSummary {
    pub created: u64,
    pub skipped: u64,
    pub closed_locally: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerSummary {
    pub pushed: u64,
    pub failed: u64,
    pub needs_choice: u64,
    /// 완료가 되돌려져 이슈를 닫지 않고 정리만 한 행 수.
    pub skipped: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinearStatus {
    pub configured: bool,
    /// false 면 키를 읽을 수 없는 기기다. `configured` 가 false 인 이유를 가른다.
    pub key_store_available: bool,
    pub pending: i64,
    pub failing: i64,
    pub needs_choice: Vec<PendingChoice>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PendingChoice {
    pub team_id: String,
    pub candidate_states: Vec<WorkflowState>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct GraphqlEnvelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Debug, Deserialize)]
struct GraphqlErrors {
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
    extensions: Option<GraphqlErrorExtensions>,
}

impl GraphqlError {
    fn is_rate_limited(&self) -> bool {
        self.extensions
            .as_ref()
            .is_some_and(|extensions| extensions.code == "RATELIMITED")
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlErrorExtensions {
    #[serde(default)]
    code: String,
}

#[derive(Debug, Deserialize)]
struct ViewerData {
    viewer: Viewer,
}

#[derive(Debug, Deserialize)]
struct Viewer {
    id: String,
}

#[derive(Debug, Deserialize)]
struct IssueData {
    issue: Option<LinearIssue>,
}

#[derive(Debug, Deserialize)]
struct IssuesData {
    issues: IssueConnection,
}

#[derive(Debug, Deserialize)]
struct IssueConnection {
    nodes: Vec<LinearIssue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinearIssue {
    id: String,
    identifier: String,
    #[serde(default)]
    title: String,
    description: Option<String>,
    url: String,
    #[serde(default)]
    priority: i64,
    #[serde(default)]
    state: RemoteState,
    team: Team,
}

impl LinearIssue {
    fn as_link_input(&self) -> LinearLinkInput {
        LinearLinkInput {
            issue_id: self.id.clone(),
            identifier: self.identifier.clone(),
            url: self.url.clone(),
            team_id: self.team.id.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RemoteState {
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct Team {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowStatesData {
    workflow_states: WorkflowStateConnection,
}

#[derive(Debug, Deserialize)]
struct WorkflowStateConnection {
    nodes: Vec<WorkflowState>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueUpdateData {
    issue_update: IssueUpdatePayload,
}

#[derive(Debug, Deserialize)]
struct IssueUpdatePayload {
    success: bool,
}

#[derive(Debug, FromRow)]
struct OutboxRow {
    id: i64,
    attempts: i64,
    issue_id: String,
    team_id: String,
    /// 아웃박스 행이 아니라 지금 이 todo 의 상태다. 큐에 담긴 뒤 완료가
    /// 되돌려졌는지 push 직전에 확인하려고 함께 읽는다.
    status: String,
}

#[derive(Debug, FromRow)]
struct LinkedActiveTodo {
    todo_id: String,
    issue_id: String,
}

#[derive(Debug, FromRow)]
struct CountRow {
    pending: i64,
    failing: i64,
}

enum ProcessOutcome {
    Pushed,
    NeedsChoice,
    Skipped,
}

async fn complete_outbox_row(
    transaction: &mut Transaction<'_, Sqlite>,
    outbox_id: i64,
    issue_id: &str,
) -> Result<(), Error> {
    let now = now_string();
    sqlx::query("UPDATE sync_outbox SET completed_at = ?, last_error = NULL WHERE id = ?")
        .bind(&now)
        .bind(outbox_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("UPDATE linear_links SET last_pushed_status = 'done' WHERE issue_id = ?")
        .bind(issue_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn get_setting(core: &TodoCore, key: &str) -> Result<Option<String>, Error> {
    Ok(
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(core.pool())
            .await?,
    )
}

async fn set_setting(core: &TodoCore, key: &str, value: &str) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(core.pool())
    .await?;
    Ok(())
}

fn issue_identifier(issue_ref: &str) -> Result<String, Error> {
    let trimmed = issue_ref.trim().trim_end_matches('/');
    let candidate = if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        let segments = trimmed.split('/').collect::<Vec<_>>();
        segments
            .windows(2)
            .find_map(|pair| (pair[0] == "issue").then_some(pair[1]))
            .ok_or_else(|| Error::InvalidInput("invalid Linear issue URL".to_owned()))?
    } else {
        trimmed
    };
    let Some((team, number)) = candidate.split_once('-') else {
        return Err(Error::InvalidInput(
            "issue_ref must be an identifier or Linear issue URL".to_owned(),
        ));
    };
    let valid_team = !team.is_empty()
        && team
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
    let valid_number =
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) && number != "0";
    if !valid_team || !valid_number || candidate.matches('-').count() != 1 {
        return Err(Error::InvalidInput(
            "issue_ref must be an identifier or Linear issue URL".to_owned(),
        ));
    }
    Ok(candidate.to_owned())
}

fn priority_from_linear(value: i64) -> Result<Priority, Error> {
    match value {
        0 => Ok(Priority::None),
        1 => Ok(Priority::Urgent),
        2 => Ok(Priority::High),
        3 => Ok(Priority::Medium),
        4 => Ok(Priority::Low),
        _ => Err(Error::Remote {
            message: "Linear returned an invalid priority".to_owned(),
            retryable: false,
        }),
    }
}

fn backoff_for_attempt(attempt: i64) -> TimeDelta {
    let seconds = match attempt {
        i64::MIN..=1 => 5,
        2 => 15,
        3 => 60,
        4 => 300,
        _ => 1_800,
    };
    TimeDelta::seconds(seconds)
}

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

const VIEWER_QUERY: &str = "query Viewer { viewer { id } }";
const ISSUE_QUERY: &str = "query ResolveIssue($issueRef: String!) { issue(id: $issueRef) { id identifier title description url priority state { id name type } team { id } } }";
const PULL_QUERY: &str = "query PullInProgress($viewerId: ID!) { issues(filter: { assignee: { id: { eq: $viewerId } }, state: { type: { eq: \"started\" } } }) { nodes { id identifier title description url priority state { id name type } team { id } } } }";
const REVERSE_STATES_QUERY: &str = "query ReverseStates($issueIds: [ID!]!) { issues(filter: { id: { in: $issueIds } }) { nodes { id identifier url state { type } team { id } } } }";
const WORKFLOW_STATES_QUERY: &str = "query CompletedStates($teamId: ID!) { workflowStates(filter: { team: { id: { eq: $teamId } }, type: { eq: \"completed\" } }) { nodes { id name } } }";
const ISSUE_UPDATE_MUTATION: &str = "mutation CompleteIssue($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success } }";
