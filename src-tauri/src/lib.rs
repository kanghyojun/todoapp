use std::{
    error::Error as StdError,
    fs,
    sync::{Arc, RwLock},
};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize};
use tauri::{Emitter, Manager, State};
use todo_core::{
    CreateTodoInput, DomainEvent, Error as CoreError, Priority, Status, Todo, TodoCore, TodoFilter,
    TodoId, TodoPatch, parse_due_date,
};
use todo_linear::{Error as LinearError, LinearService, PullSummary, SystemKeyStore};
use todo_server::{
    ServerConfig, StartupError, bind, build_router_with_linear, default_token_path,
    load_or_create_token, serve,
};

const SERVER_PORT: u16 = 2470;
const CHANGED_EVENT: &str = "todo:changed";
const SERVER_STATUS_EVENT: &str = "todo:server-status";

#[derive(Clone)]
struct ShellState {
    core: TodoCore,
    linear: LinearService,
    server_status: SharedServerStatus,
}

#[derive(Clone)]
struct SharedServerStatus(Arc<RwLock<ServerStatus>>);

impl SharedServerStatus {
    fn new(status: ServerStatus) -> Self {
        Self(Arc::new(RwLock::new(status)))
    }

    fn get(&self) -> Result<ServerStatus, CommandError> {
        self.0
            .read()
            .map(|status| status.clone())
            .map_err(|_| CommandError::internal("server status lock is poisoned"))
    }

    fn set(&self, status: ServerStatus) {
        if let Ok(mut current) = self.0.write() {
            *current = status;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ServerStatus {
    running: bool,
    error: Option<String>,
}

impl ServerStatus {
    fn running() -> Self {
        Self {
            running: true,
            error: None,
        }
    }

    fn failed(error: String) -> Self {
        Self {
            running: false,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
struct CommandError {
    code: &'static str,
    message: String,
}

impl CommandError {
    fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_input",
            message: message.into(),
        }
    }

    fn internal(detail: impl std::fmt::Display) -> Self {
        eprintln!("todo shell internal error: {detail}");
        Self {
            code: "internal_error",
            message: "the operation failed".to_owned(),
        }
    }
}

impl From<CoreError> for CommandError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::NotFound(_) => Self {
                code: "not_found",
                message: "todo not found".to_owned(),
            },
            CoreError::InvalidInput(message) => Self::invalid_input(message),
            CoreError::IssueAlreadyLinked => Self {
                code: "issue_already_linked",
                message: "the Linear issue is already linked".to_owned(),
            },
            CoreError::ParseDueDate(error) => Self::invalid_input(error.to_string()),
            CoreError::Database(detail) => Self::internal(detail),
        }
    }
}

impl From<LinearError> for CommandError {
    fn from(error: LinearError) -> Self {
        match error {
            LinearError::NotConfigured => Self {
                code: "linear_not_configured",
                message: "Linear API key is not configured".to_owned(),
            },
            LinearError::Unauthorized => Self {
                code: "linear_unauthorized",
                message: "Linear rejected the API key".to_owned(),
            },
            LinearError::IssueNotFound => Self {
                code: "not_found",
                message: "Linear issue not found".to_owned(),
            },
            LinearError::InvalidInput(message) => Self::invalid_input(message),
            LinearError::Core(error) => error.into(),
            LinearError::Remote { .. } => Self {
                code: "linear_api_error",
                message: "the Linear API request failed".to_owned(),
            },
            LinearError::Database(detail) => Self::internal(detail),
            LinearError::KeyStore(error) => {
                eprintln!("todo shell key store error: {error}");
                Self {
                    code: "key_store_error",
                    message: "the OS keychain is unavailable".to_owned(),
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRequest {
    status: Option<Status>,
    priority: Option<Priority>,
    due_before: Option<String>,
    q: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl ListRequest {
    fn into_core(self) -> Result<TodoFilter, CommandError> {
        Ok(TodoFilter {
            status: self.status,
            priority: self.priority,
            due_before: self.due_before.as_deref().map(parse_iso_date).transpose()?,
            query: self.q,
            limit: self.limit,
            offset: self.offset,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    title: String,
    #[serde(default)]
    description: String,
    status: Option<Status>,
    #[serde(default)]
    priority: Priority,
    due_date: Option<String>,
}

impl CreateRequest {
    fn into_core(self) -> Result<CreateTodoInput, CommandError> {
        let due_date = self
            .due_date
            .as_deref()
            .map(|input| parse_due_date(input, Local::now().date_naive()))
            .transpose()
            .map_err(CoreError::from)?
            .flatten();
        Ok(CreateTodoInput {
            title: self.title,
            description: self.description,
            status: self.status.unwrap_or(Status::Todo),
            priority: self.priority,
            due_date,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    title: Option<String>,
    description: Option<String>,
    status: Option<Status>,
    priority: Option<Priority>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    due_date: Option<Option<String>>,
}

impl From<UpdateRequest> for TodoPatch {
    fn from(request: UpdateRequest) -> Self {
        Self {
            title: request.title,
            description: request.description,
            status: request.status,
            priority: request.priority,
            due_date: request.due_date,
        }
    }
}

fn deserialize_optional_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn parse_todo_id(value: &str) -> Result<TodoId, CommandError> {
    value
        .parse()
        .map_err(|_| CommandError::invalid_input("id must be a UUID"))
}

fn parse_iso_date(value: &str) -> Result<NaiveDate, CommandError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| CommandError::invalid_input("date must use YYYY-MM-DD format"))
}

#[tauri::command]
async fn list(
    filter: ListRequest,
    state: State<'_, ShellState>,
) -> Result<Vec<Todo>, CommandError> {
    Ok(state.core.list_todos(filter.into_core()?).await?)
}

#[tauri::command]
async fn get(id: String, state: State<'_, ShellState>) -> Result<Todo, CommandError> {
    Ok(state.core.get_todo(parse_todo_id(&id)?).await?)
}

#[tauri::command]
async fn create(input: CreateRequest, state: State<'_, ShellState>) -> Result<Todo, CommandError> {
    Ok(state.core.create_todo(input.into_core()?).await?)
}

#[tauri::command]
async fn update(
    id: String,
    patch: UpdateRequest,
    state: State<'_, ShellState>,
) -> Result<Todo, CommandError> {
    Ok(state
        .core
        .update_todo(parse_todo_id(&id)?, patch.into())
        .await?)
}

#[tauri::command]
async fn set_status(
    id: String,
    status: Status,
    state: State<'_, ShellState>,
) -> Result<Todo, CommandError> {
    Ok(state.core.set_status(parse_todo_id(&id)?, status).await?)
}

#[tauri::command]
async fn delete(id: String, state: State<'_, ShellState>) -> Result<(), CommandError> {
    Ok(state.core.delete_todo(parse_todo_id(&id)?).await?)
}

#[tauri::command]
async fn restore(id: String, state: State<'_, ShellState>) -> Result<Todo, CommandError> {
    Ok(state.core.restore_todo(parse_todo_id(&id)?).await?)
}

#[tauri::command]
async fn link_linear(
    id: String,
    issue_ref: String,
    state: State<'_, ShellState>,
) -> Result<(), CommandError> {
    Ok(state.linear.link(parse_todo_id(&id)?, &issue_ref).await?)
}

#[tauri::command]
async fn pull_linear(state: State<'_, ShellState>) -> Result<PullSummary, CommandError> {
    Ok(state.linear.pull().await?)
}

#[tauri::command]
fn server_status(state: State<'_, ShellState>) -> Result<ServerStatus, CommandError> {
    state.server_status.get()
}

async fn initialize(app: tauri::AppHandle) -> Result<ShellState, Box<dyn StdError>> {
    let database_dir = app.path().data_dir()?.join("todo");
    fs::create_dir_all(&database_dir)?;
    let database_url = format!("sqlite://{}", database_dir.join("todo.db").display());
    let core = TodoCore::connect(&database_url).await?;
    let linear = LinearService::new(core.clone(), Arc::new(SystemKeyStore));

    forward_domain_events(app.clone(), core.subscribe());

    // 아웃박스 워커는 서버와 무관하다. 포트가 막혀 REST/MCP 가 안 떠도
    // done 을 누르면 Linear 로 밀려야 한다.
    linear.spawn_worker();

    let home = app.path().home_dir()?;
    let token = load_or_create_token(&default_token_path(&home))?;
    let router = build_router_with_linear(
        core.clone(),
        linear.clone(),
        ServerConfig {
            port: SERVER_PORT,
            token,
            dev_origins: Vec::new(),
        },
    )?;

    let server_status = match bind(SERVER_PORT).await {
        Ok(listener) => {
            let status = SharedServerStatus::new(ServerStatus::running());
            let task_status = status.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = serve(listener, router).await {
                    eprintln!("todo external server stopped: {error}");
                    let next = ServerStatus::failed(error.to_string());
                    task_status.set(next.clone());
                    let _ = app.emit(SERVER_STATUS_EVENT, next);
                }
            });
            status
        }
        Err(error) => {
            let message = server_startup_message(&error);
            eprintln!("todo external server unavailable: {error}");
            SharedServerStatus::new(ServerStatus::failed(message))
        }
    };

    Ok(ShellState {
        core,
        linear,
        server_status,
    })
}

fn forward_domain_events(
    app: tauri::AppHandle,
    mut events: tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = app.emit(CHANGED_EVENT, ());
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn server_startup_message(error: &StartupError) -> String {
    match error {
        StartupError::PortInUse { port } => format!("{port} 포트가 사용 중입니다"),
        _ => error.to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(initialize(app.handle().clone()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list,
            get,
            create,
            update,
            set_status,
            delete,
            restore,
            link_linear,
            pull_linear,
            server_status
        ])
        .run(tauri::generate_context!())
        .expect("failed to run todo desktop app");
}
