use std::{str::FromStr, time::Duration};

use chrono::{Local, NaiveDate, SecondsFormat, Utc};
use sqlx::{
    FromRow, QueryBuilder, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::sync::broadcast;

use crate::{
    CreateTodoInput, DomainEvent, Error, LinearLinkInput, LinearRef, Priority, Result, Status,
    Todo, TodoFilter, TodoId, TodoPatch, parse_due_date,
};

#[derive(Clone)]
pub struct TodoCore {
    pool: SqlitePool,
    events: broadcast::Sender<DomainEvent>,
}

impl TodoCore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(5_000))
            .synchronous(SqliteSynchronous::Normal);
        let is_in_memory = is_in_memory_url(database_url);
        let max_connections = if is_in_memory { 1 } else { 5 };
        let pool_options = SqlitePoolOptions::new().max_connections(max_connections);
        let pool_options = if is_in_memory {
            pool_options.idle_timeout(None).max_lifetime(None)
        } else {
            pool_options
        };
        let pool = pool_options.connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let (events, _) = broadcast::channel(128);
        Ok(Self { pool, events })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.events.subscribe()
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn create_todo(&self, input: CreateTodoInput) -> Result<Todo> {
        let title = normalize_title(input.title)?;
        let id = TodoId::new();
        let now = now_string();
        let completed_at = (input.status == Status::Done).then_some(now.as_str());
        let due_date = input
            .due_date
            .map(|date| date.format("%Y-%m-%d").to_string());
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, TodoRow>(
            "INSERT INTO todos \
             (id, title, description, status, priority, due_date, completed_at, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING id, title, description, status, priority, due_date, completed_at, \
                       created_at, updated_at, deleted_at",
        )
        .bind(id.to_string())
        .bind(title)
        .bind(input.description)
        .bind(input.status.as_db_str())
        .bind(input.priority.as_db_integer())
        .bind(due_date)
        .bind(completed_at)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *transaction)
        .await?;
        let todo = row.try_into()?;
        transaction.commit().await?;

        let _ = self.events.send(DomainEvent::TodoCreated(id));
        Ok(todo)
    }

    pub async fn get_todo(&self, id: TodoId) -> Result<Todo> {
        let row = sqlx::query_as::<_, TodoRow>(
            "SELECT t.id, t.title, t.description, t.status, t.priority, t.due_date, \
                    t.completed_at, t.created_at, t.updated_at, t.deleted_at, \
                    t.deferred_until, \
                    li.identifier AS linear_identifier, li.url AS linear_url \
             FROM todos AS t LEFT JOIN linear_links AS li ON li.todo_id = t.id \
             WHERE t.id = ? AND t.deleted_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound(id))?;
        row.try_into()
    }

    pub async fn list_todos(&self, filter: TodoFilter) -> Result<Vec<Todo>> {
        // 읽기가 깨운다. 복귀일이 지난 보류 항목을 먼저 todo 로 되돌린다.
        // 그래야 어느 클라이언트로 읽어도 똑같이 깨어난다.
        self.wake_due_deferred().await?;

        let TodoFilter {
            status,
            priority,
            due_before,
            query: search,
            limit,
            offset,
        } = filter;
        let match_expression = if let Some(search) = search.as_deref() {
            let Some(expression) = fts_match_expression(search) else {
                return Ok(Vec::new());
            };
            Some(expression)
        } else {
            None
        };
        let mut query = if let Some(expression) = match_expression {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT t.id, t.title, t.description, t.status, t.priority, t.due_date, \
                 t.completed_at, t.created_at, t.updated_at, t.deleted_at, \
                 t.deferred_until, \
                 li.identifier AS linear_identifier, li.url AS linear_url \
                 FROM todos_fts JOIN todos AS t ON t.rowid = todos_fts.rowid \
                 LEFT JOIN linear_links AS li ON li.todo_id = t.id \
                 WHERE todos_fts MATCH ",
            );
            query
                .push_bind(expression)
                .push(" AND t.deleted_at IS NULL");
            query
        } else {
            QueryBuilder::<Sqlite>::new(
                "SELECT t.id, t.title, t.description, t.status, t.priority, t.due_date, \
                 t.completed_at, t.created_at, t.updated_at, t.deleted_at, \
                 t.deferred_until, \
                 li.identifier AS linear_identifier, li.url AS linear_url \
                 FROM todos AS t LEFT JOIN linear_links AS li ON li.todo_id = t.id \
                 WHERE t.deleted_at IS NULL",
            )
        };
        let column_prefix = if search.is_some() { "t." } else { "" };
        if let Some(status) = status {
            query
                .push(" AND ")
                .push(column_prefix)
                .push("status = ")
                .push_bind(status.as_db_str());
        } else {
            // 상태를 안 고르면(전체) 보류는 뺀다. 보류는 별도 레인으로만 본다.
            query
                .push(" AND ")
                .push(column_prefix)
                .push("status != 'deferred'");
        }
        if let Some(priority) = priority {
            query
                .push(" AND ")
                .push(column_prefix)
                .push("priority = ")
                .push_bind(priority.as_db_integer());
        }
        if let Some(due_before) = due_before {
            query
                .push(" AND ")
                .push(column_prefix)
                .push("due_date < ")
                .push_bind(due_before.format("%Y-%m-%d").to_string());
        }
        if search.is_some() {
            query.push(" ORDER BY bm25(todos_fts), t.created_at ASC, t.id ASC");
        } else {
            query.push(
                " ORDER BY CASE status \
                   WHEN 'in_progress' THEN 0 WHEN 'todo' THEN 1 ELSE 2 END, \
                   due_date IS NULL, due_date ASC, priority_rank ASC, created_at ASC, id ASC",
            );
        }
        match (limit, offset) {
            (Some(limit), offset) => {
                query.push(" LIMIT ").push_bind(i64::from(limit.min(1_000)));
                if let Some(offset) = offset {
                    query.push(" OFFSET ").push_bind(i64::from(offset));
                }
            }
            (None, Some(offset)) => {
                query.push(" LIMIT -1 OFFSET ").push_bind(i64::from(offset));
            }
            (None, None) => {}
        }
        let rows = query
            .build_query_as::<TodoRow>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    /// 복귀일이 오늘이거나 지난 보류 항목을 todo 로 되돌린다.
    /// 무기한(deferred_until IS NULL) 보류는 건드리지 않는다.
    ///
    /// 읽기마다 불린다. 그래서 먼저 읽기로 깨울 게 있는지 본 다음,
    /// 있을 때만 쓴다. 평소엔 깨울 게 없어 쓰기 락을 안 잡는다. 안 그러면
    /// 동시 list 호출들이 WAL 쓰기 락을 두고 경합해 커넥션 풀이 막힌다.
    async fn wake_due_deferred(&self) -> Result<()> {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let has_due = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS( \
               SELECT 1 FROM todos \
               WHERE status = 'deferred' AND deferred_until IS NOT NULL \
                 AND deferred_until <= ? AND deleted_at IS NULL)",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await?;
        if has_due == 0 {
            return Ok(());
        }
        sqlx::query(
            "UPDATE todos SET status = 'todo', deferred_until = NULL, updated_at = ? \
             WHERE status = 'deferred' AND deferred_until IS NOT NULL \
               AND deferred_until <= ? AND deleted_at IS NULL",
        )
        .bind(now_string())
        .bind(today)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 할 일을 보류로 보낸다. `input` 은 복귀일을 자연어로 받는다(마감일과
    /// 같은 파서). 비면 무기한이다. 보류는 done 이 아니므로 완료 시각을
    /// 지우고 아웃박스에 아무것도 넣지 않는다. 삭제된 항목은 복구 외 어떤
    /// 변경도 받지 않는다.
    pub async fn defer_todo(&self, id: TodoId, input: &str) -> Result<Todo> {
        let until = parse_due_date(input, Local::now().date_naive())
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let mut transaction = self.pool.begin().await?;
        fetch_todo_row(id, &mut transaction).await?;
        let now = now_string();
        let deferred_until = until.map(|date| date.format("%Y-%m-%d").to_string());
        let updated = sqlx::query(
            "UPDATE todos SET status = 'deferred', deferred_until = ?, \
             completed_at = NULL, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(deferred_until)
        .bind(&now)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(Error::NotFound(id));
        }
        let todo = fetch_todo_row(id, &mut transaction).await?.try_into()?;
        transaction.commit().await?;

        self.emit(DomainEvent::TodoUpdated(id));
        Ok(todo)
    }

    pub async fn update_todo(&self, id: TodoId, patch: TodoPatch) -> Result<Todo> {
        let mut transaction = self.pool.begin().await?;
        let current = fetch_todo_row(id, &mut transaction).await?;
        let now = now_string();
        let patched_title = patch.title.map(normalize_title).transpose()?;
        let title = patched_title.unwrap_or(current.title);
        let description = patch.description.unwrap_or(current.description);
        let status = patch
            .status
            .map_or_else(|| Status::from_db(&current.status), Ok)?;
        let priority = patch
            .priority
            .map_or(current.priority, Priority::as_db_integer);
        let due_date = match patch.due_date {
            Some(Some(input)) => parse_due_date(&input, Local::now().date_naive())
                .map_err(|error| Error::InvalidInput(error.to_string()))?
                .map(format_date),
            Some(None) => None,
            None => current.due_date,
        };
        let completed_at = match patch.status {
            Some(Status::Done) => Some(now.clone()),
            Some(_) => None,
            None => current.completed_at,
        };
        // 보류로 남으면(예: 보류 항목의 제목만 편집) 복귀일을 지킨다.
        // 보류를 벗어나면 항상 복귀일을 지운다. 보류로 보내는 것은
        // update_todo 가 아니라 defer_todo 가 한다.
        let deferred_until = if status == Status::Deferred {
            current.deferred_until
        } else {
            None
        };
        let updated = sqlx::query(
            "UPDATE todos SET title = ?, description = ?, status = ?, priority = ?, due_date = ?, \
             completed_at = ?, deferred_until = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(title)
        .bind(description)
        .bind(status.as_db_str())
        .bind(priority)
        .bind(due_date)
        .bind(completed_at)
        .bind(deferred_until)
        .bind(&now)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(Error::NotFound(id));
        }
        let outbox_inserted = if patch.status == Some(Status::Done) {
            sqlx::query(
                "INSERT OR IGNORE INTO sync_outbox \
                 (todo_id, kind, next_attempt_at, created_at) \
                 SELECT ?, 'linear_complete', ?, ? \
                 WHERE EXISTS (SELECT 1 FROM linear_links WHERE todo_id = ?)",
            )
            .bind(id.to_string())
            .bind(&now)
            .bind(&now)
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected()
                == 1
        } else {
            false
        };
        let todo = fetch_todo_row(id, &mut transaction).await?.try_into()?;
        transaction.commit().await?;

        self.emit(DomainEvent::TodoUpdated(id));
        if outbox_inserted {
            self.emit(DomainEvent::SyncStateChanged);
        }
        Ok(todo)
    }

    pub async fn set_status(&self, id: TodoId, status: Status) -> Result<Todo> {
        self.update_todo(
            id,
            TodoPatch {
                status: Some(status),
                ..TodoPatch::default()
            },
        )
        .await
    }

    pub async fn mark_done_from_remote(&self, id: TodoId) -> Result<Todo> {
        let mut transaction = self.pool.begin().await?;
        fetch_todo_row(id, &mut transaction).await?;
        let now = now_string();
        let updated = sqlx::query(
            "UPDATE todos SET status = 'done', completed_at = ?, deferred_until = NULL, \
             updated_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(Error::NotFound(id));
        }
        let todo = fetch_todo_row(id, &mut transaction).await?.try_into()?;
        transaction.commit().await?;

        self.emit(DomainEvent::TodoUpdated(id));
        Ok(todo)
    }

    pub async fn delete_todo(&self, id: TodoId) -> Result<()> {
        self.set_deleted_at(id, Some(now_string())).await?;
        self.emit(DomainEvent::TodoDeleted(id));
        Ok(())
    }

    pub async fn restore_todo(&self, id: TodoId) -> Result<Todo> {
        self.set_deleted_at(id, None).await?;
        let todo = self.get_todo(id).await?;
        self.emit(DomainEvent::TodoRestored(id));
        Ok(todo)
    }

    pub async fn link_linear(&self, id: TodoId, link: LinearLinkInput) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            "INSERT INTO linear_links \
             (todo_id, issue_id, identifier, url, team_id, linked_at) \
             SELECT ?, ?, ?, ?, ?, ? FROM todos \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id.to_string())
        .bind(link.issue_id)
        .bind(link.identifier)
        .bind(link.url)
        .bind(link.team_id)
        .bind(now_string())
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await;
        let inserted = match result {
            Ok(inserted) => inserted,
            Err(error) => {
                if error
                    .as_database_error()
                    .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
                {
                    return Err(Error::IssueAlreadyLinked);
                }
                return Err(error.into());
            }
        };
        if inserted.rows_affected() == 0 {
            return Err(Error::NotFound(id));
        }
        transaction.commit().await?;
        self.emit(DomainEvent::TodoUpdated(id));
        Ok(())
    }

    async fn set_deleted_at(&self, id: TodoId, deleted_at: Option<String>) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query("UPDATE todos SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(deleted_at)
            .bind(now_string())
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(Error::NotFound(id));
        }
        transaction.commit().await?;
        Ok(())
    }

    fn emit(&self, event: DomainEvent) {
        let _ = self.events.send(event);
    }
}

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn is_in_memory_url(database_url: &str) -> bool {
    let url = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);
    let (database, parameters) = url.split_once('?').unwrap_or((url, ""));
    database == ":memory:"
        || parameters
            .split('&')
            .any(|parameter| parameter == "mode=memory")
}

fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn normalize_title(title: String) -> Result<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("title must not be blank".to_owned()));
    }
    Ok(trimmed.to_owned())
}

fn fts_match_expression(input: &str) -> Option<String> {
    let tokens: Vec<_> = input.split_whitespace().collect();
    let last_index = tokens.len().checked_sub(1)?;
    Some(
        tokens
            .into_iter()
            .enumerate()
            .map(|(index, token)| {
                let escaped = token.replace('"', "\"\"");
                let prefix = if index == last_index { "*" } else { "" };
                format!("\"{escaped}\"{prefix}")
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

async fn fetch_todo_row(
    id: TodoId,
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
) -> Result<TodoRow> {
    sqlx::query_as::<_, TodoRow>(
        "SELECT t.id, t.title, t.description, t.status, t.priority, t.due_date, \
                t.completed_at, t.created_at, t.updated_at, t.deleted_at, \
                t.deferred_until, \
                li.identifier AS linear_identifier, li.url AS linear_url \
         FROM todos AS t LEFT JOIN linear_links AS li ON li.todo_id = t.id \
         WHERE t.id = ? AND t.deleted_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(Error::NotFound(id))
}

#[derive(Debug, FromRow)]
struct TodoRow {
    id: String,
    title: String,
    description: String,
    status: String,
    priority: i64,
    due_date: Option<String>,
    completed_at: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    #[sqlx(default)]
    deferred_until: Option<String>,
    #[sqlx(default)]
    linear_identifier: Option<String>,
    #[sqlx(default)]
    linear_url: Option<String>,
}

impl TryFrom<TodoRow> for Todo {
    type Error = Error;

    fn try_from(row: TodoRow) -> Result<Self> {
        let id = TodoId::from_str(&row.id).map_err(|error| {
            Error::InvalidInput(format!("invalid todo id in database: {error}"))
        })?;
        validate_date(&row.due_date)?;
        Ok(Self {
            id,
            title: row.title,
            description: row.description,
            status: Status::from_db(&row.status)?,
            priority: Priority::from_db(row.priority)?,
            due_date: row.due_date,
            completed_at: row.completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            deferred_until: row.deferred_until,
            linear: match (row.linear_identifier, row.linear_url) {
                (Some(identifier), Some(url)) => Some(LinearRef { identifier, url }),
                _ => None,
            },
        })
    }
}

fn validate_date(value: &Option<String>) -> Result<()> {
    if let Some(value) = value {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| {
            Error::InvalidInput(format!("invalid due date in database: {error}"))
        })?;
    }
    Ok(())
}
