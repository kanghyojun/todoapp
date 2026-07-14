use std::{
    collections::HashMap,
    error::Error as StdError,
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::{Arc, RwLock},
};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize};
use tauri::{Emitter, Manager, State};
use todo_core::{
    CreateTodoInput, DomainEvent, EmailLinkInput, Error as CoreError, Priority, Status, Todo,
    TodoCore, TodoFilter, TodoId, TodoPatch, parse_due_date,
};
use todo_gmail::{
    Error as GmailError, GmailAccount, GmailService, MailBody, MailFilter, MailFolder,
    MailListItem, SystemTokenStore,
};
use todo_linear::{Error as LinearError, LinearService, LinearStatus, PullSummary, SystemKeyStore};
use todo_server::{
    ServerConfig, StartupError, bind_local_and, build_router_with_linear, default_config_path,
    default_token_path, load_or_create_token, read_bind_config, serve,
};

const SERVER_PORT: u16 = 2470;
const CHANGED_EVENT: &str = "todo:changed";
const MAIL_CHANGED_EVENT: &str = "mail:changed";
const SERVER_STATUS_EVENT: &str = "todo:server-status";

#[derive(Clone)]
struct ShellState {
    core: TodoCore,
    linear: LinearService,
    gmail: GmailService,
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

impl From<GmailError> for CommandError {
    fn from(error: GmailError) -> Self {
        match error {
            GmailError::NotConfigured => Self {
                code: "gmail_not_configured",
                message: "Gmail is not configured".to_owned(),
            },
            GmailError::Unauthorized => Self {
                code: "gmail_unauthorized",
                message: "Google rejected the credentials".to_owned(),
            },
            GmailError::AccountNotFound | GmailError::MessageNotFound => Self {
                code: "not_found",
                message: "the requested item was not found".to_owned(),
            },
            GmailError::InvalidInput(message) => Self::invalid_input(message),
            GmailError::HistoryExpired => Self {
                code: "gmail_api_error",
                message: "Gmail history is too old to replay".to_owned(),
            },
            GmailError::Remote { .. } => Self {
                code: "gmail_api_error",
                message: "the Gmail API request failed".to_owned(),
            },
            GmailError::Database(detail) => Self::internal(detail),
            GmailError::TokenStore(error) => {
                eprintln!("todo shell gmail token store error: {error}");
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateFromEmailRequest {
    title: String,
    account_id: String,
    gmail_id: String,
    thread_id: String,
    subject: String,
    from_name: String,
    from_email: String,
}

#[tauri::command]
async fn create_todo_from_email(
    input: CreateFromEmailRequest,
    state: State<'_, ShellState>,
) -> Result<Todo, CommandError> {
    let CreateFromEmailRequest {
        title,
        account_id,
        gmail_id,
        thread_id,
        subject,
        from_name,
        from_email,
    } = input;
    let email = EmailLinkInput {
        account_id,
        gmail_id,
        thread_id,
        subject,
        from_name,
        from_email,
    };
    Ok(state.core.create_todo_from_email(title, email).await?)
}

#[tauri::command]
async fn defer(
    id: String,
    until: String,
    state: State<'_, ShellState>,
) -> Result<Todo, CommandError> {
    Ok(state.core.defer_todo(parse_todo_id(&id)?, &until).await?)
}

#[tauri::command]
async fn pull_linear(state: State<'_, ShellState>) -> Result<PullSummary, CommandError> {
    Ok(state.linear.pull().await?)
}

#[tauri::command]
async fn linear_status(state: State<'_, ShellState>) -> Result<LinearStatus, CommandError> {
    Ok(state.linear.status().await?)
}

#[tauri::command]
async fn set_linear_key(api_key: String, state: State<'_, ShellState>) -> Result<(), CommandError> {
    Ok(state.linear.set_api_key(&api_key).await?)
}

#[tauri::command]
fn server_status(state: State<'_, ShellState>) -> Result<ServerStatus, CommandError> {
    state.server_status.get()
}

#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    // opener 의 기본 권한이 http/https/mailto/tel 로 스킴을 제한한다.
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(CommandError::internal)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GmailListRequest {
    folder: String,
    account_id: Option<String>,
    q: Option<String>,
    limit: Option<u32>,
}

impl GmailListRequest {
    fn into_core(self) -> Result<MailFilter, CommandError> {
        let folder = match self.folder.as_str() {
            "inbox" => MailFolder::Inbox,
            "archive" => MailFolder::Archive,
            "all" => MailFolder::All,
            other => {
                return Err(CommandError::invalid_input(format!(
                    "unknown mail folder: {other}"
                )));
            }
        };
        Ok(MailFilter {
            folder,
            account_id: self.account_id,
            query: self.q,
            limit: self.limit,
        })
    }
}

#[tauri::command]
async fn gmail_accounts(state: State<'_, ShellState>) -> Result<Vec<GmailAccount>, CommandError> {
    Ok(state.gmail.accounts().await?)
}

#[tauri::command]
async fn gmail_list(
    filter: GmailListRequest,
    state: State<'_, ShellState>,
) -> Result<Vec<MailListItem>, CommandError> {
    Ok(state.gmail.list(filter.into_core()?).await?)
}

/// 받은편지함 안읽음 개수. Mail 탭 뱃지가 이 값을 읽어 그린다.
#[tauri::command]
async fn gmail_unread_count(state: State<'_, ShellState>) -> Result<u64, CommandError> {
    Ok(state.gmail.unread_count().await?)
}

#[tauri::command]
async fn gmail_get_body(
    account_id: String,
    gmail_id: String,
    state: State<'_, ShellState>,
) -> Result<MailBody, CommandError> {
    Ok(state.gmail.get_body(&account_id, &gmail_id).await?)
}

#[tauri::command]
async fn gmail_archive(
    account_id: String,
    gmail_id: String,
    state: State<'_, ShellState>,
) -> Result<(), CommandError> {
    Ok(state.gmail.archive(&account_id, &gmail_id).await?)
}

#[tauri::command]
async fn gmail_set_read(
    account_id: String,
    gmail_id: String,
    read: bool,
    state: State<'_, ShellState>,
) -> Result<(), CommandError> {
    Ok(state.gmail.set_read(&account_id, &gmail_id, read).await?)
}

#[tauri::command]
async fn gmail_sync(state: State<'_, ShellState>) -> Result<(), CommandError> {
    Ok(state.gmail.sync_all().await?)
}

#[tauri::command]
async fn gmail_remove_account(
    account_id: String,
    state: State<'_, ShellState>,
) -> Result<(), CommandError> {
    Ok(state.gmail.remove_account(&account_id).await?)
}

#[tauri::command]
async fn gmail_set_credentials(
    client_id: String,
    client_secret: String,
    state: State<'_, ShellState>,
) -> Result<(), CommandError> {
    Ok(state
        .gmail
        .set_client_credentials(&client_id, &client_secret)
        .await?)
}

/// OAuth loopback 흐름으로 Google 계정을 추가한다. 브라우저 동의를 거친다.
#[tauri::command]
async fn gmail_add_account(state: State<'_, ShellState>) -> Result<GmailAccount, CommandError> {
    let gmail = state.gmail.clone();
    let client_id = gmail.client_id().await?.ok_or(CommandError {
        code: "gmail_not_configured",
        message: "OAuth 클라이언트를 먼저 설정하십시오".to_owned(),
    })?;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(CommandError::internal)?;
    let port = listener
        .local_addr()
        .map_err(CommandError::internal)?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let (verifier, challenge) = todo_gmail::oauth::generate_pkce();
    let expected_state = todo_gmail::oauth::random_state();
    let auth_url =
        todo_gmail::oauth::build_auth_url(&client_id, &redirect_uri, &challenge, &expected_state);
    open::that(&auth_url).map_err(CommandError::internal)?;

    let state_for_wait = expected_state.clone();
    let code =
        tauri::async_runtime::spawn_blocking(move || wait_for_code(&listener, &state_for_wait))
            .await
            .map_err(CommandError::internal)??;

    let account = gmail.complete_auth(&code, &verifier, &redirect_uri).await?;

    // 초기 동기화는 백그라운드로 돌려 계정 추가를 빠르게 끝낸다.
    let sync_gmail = gmail.clone();
    let sync_id = account.id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = sync_gmail.sync_account(&sync_id).await {
            eprintln!("todo shell initial gmail sync failed: {error}");
        }
    });
    Ok(account)
}

/// loopback 리스너에서 첫 콜백을 받아 code 를 꺼내고 state 를 검증한다.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String, CommandError> {
    let (mut stream, _) = listener.accept().map_err(CommandError::internal)?;
    let read_stream = stream.try_clone().map_err(CommandError::internal)?;
    let mut reader = BufReader::new(read_stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(CommandError::internal)?;

    let query = parse_query(&request_line);
    let code = query.get("code").cloned();
    let state_ok = query.get("state").map(String::as_str) == Some(expected_state);
    let ok = code.is_some() && state_ok;
    let body = if ok {
        "<html><body>로그인이 완료되었습니다. 이 창을 닫아도 됩니다.</body></html>"
    } else {
        "<html><body>인증에 실패했습니다. 앱으로 돌아가 다시 시도하십시오.</body></html>"
    };
    let status = if ok { "200 OK" } else { "400 Bad Request" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();

    match code {
        Some(code) if state_ok => Ok(code),
        _ => Err(CommandError {
            code: "gmail_auth_failed",
            message: "OAuth 콜백 검증에 실패했습니다".to_owned(),
        }),
    }
}

fn parse_query(request_line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(target) = request_line.split_whitespace().nth(1) else {
        return map;
    };
    let Some((_, query)) = target.split_once('?') else {
        return map;
    };
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            map.insert(key.to_owned(), urldecode(value));
        }
    }
    map
}

fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn initialize(app: tauri::AppHandle) -> Result<ShellState, Box<dyn StdError>> {
    let database_dir = app.path().data_dir()?.join("todo");
    fs::create_dir_all(&database_dir)?;
    let database_url = format!("sqlite://{}", database_dir.join("todo.db").display());
    let core = TodoCore::connect(&database_url).await?;
    let linear = LinearService::new(core.clone(), Arc::new(SystemKeyStore));
    let gmail = GmailService::new(core.clone(), Arc::new(SystemTokenStore));

    forward_domain_events(app.clone(), core.subscribe());
    forward_mail_events(app.clone(), gmail.clone(), gmail.subscribe());
    // 창이 뜰 때 이미 안읽음이 있으면 첫 동기화를 기다리지 않고 바로 Dock 에 올린다.
    refresh_dock_badge(&app, &gmail).await;

    // 아웃박스 워커는 서버와 무관하다. 포트가 막혀 REST/MCP 가 안 떠도
    // done 을 누르면 Linear 로 밀려야 한다.
    linear.spawn_worker();
    // Gmail 동기화·아웃박스 워커.
    gmail.spawn_workers();

    let home = app.path().home_dir()?;
    let bind_addr = read_bind_config(&default_config_path(&home));
    let token = load_or_create_token(&default_token_path(&home))?;
    let router = build_router_with_linear(
        core.clone(),
        linear.clone(),
        ServerConfig {
            bind: bind_addr,
            port: SERVER_PORT,
            token,
            dev_origins: Vec::new(),
        },
    )?;

    let server_status = match bind_local_and(bind_addr, SERVER_PORT).await {
        Ok(listeners) => {
            // 루프백은 항상, 추가 주소(Tailscale IP 등)는 열렸으면 함께 같은
            // 라우터로 서빙한다. 어느 리스너가 죽든 상태를 failed 로 알린다.
            let status = SharedServerStatus::new(ServerStatus::running());
            for listener in listeners {
                let router = router.clone();
                let task_status = status.clone();
                let task_app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = serve(listener, router).await {
                        eprintln!("todo external server stopped: {error}");
                        let next = ServerStatus::failed(error.to_string());
                        task_status.set(next.clone());
                        let _ = task_app.emit(SERVER_STATUS_EVENT, next);
                    }
                });
            }
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
        gmail,
        server_status,
    })
}

fn forward_domain_events(
    app: tauri::AppHandle,
    mut events: tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    tauri::async_runtime::spawn(async move {
        // Closed 면 끝난다. Lagged 는 이벤트를 놓쳤을 뿐이니 한 번 알리고 잇는다.
        while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            events.recv().await
        {
            let _ = app.emit(CHANGED_EVENT, ());
        }
    });
}

fn forward_mail_events(
    app: tauri::AppHandle,
    gmail: GmailService,
    mut events: tokio::sync::broadcast::Receiver<todo_gmail::MailEvent>,
) {
    tauri::async_runtime::spawn(async move {
        // Closed 면 끝난다. Lagged 는 이벤트를 놓쳤을 뿐이니 한 번 알리고 잇는다.
        while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            events.recv().await
        {
            let _ = app.emit(MAIL_CHANGED_EVENT, ());
            // 메일이 바뀔 때마다 Dock 카운트를 다시 맞춘다. 웹뷰와 무관하게
            // 백엔드가 뱃지의 단일 출처다.
            refresh_dock_badge(&app, &gmail).await;
        }
    });
}

/// 받은편지함 안읽음 개수를 macOS Dock 뱃지에 반영한다. 0 이면 뱃지를 지운다.
/// 부가 정보라 실패는 삼키고 앱을 막지 않는다. 비 macOS 에서는 아무것도 안 한다.
async fn refresh_dock_badge(app: &tauri::AppHandle, gmail: &GmailService) {
    #[cfg(target_os = "macos")]
    {
        let count = match gmail.unread_count().await {
            Ok(count) => count,
            Err(error) => {
                eprintln!("todo dock badge: unread count failed: {error}");
                return;
            }
        };
        if let Some(window) = app.get_webview_window("main") {
            let badge = (count > 0).then_some(count as i64);
            let _ = window.set_badge_count(badge);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, gmail);
    }
}

fn server_startup_message(error: &StartupError) -> String {
    match error {
        StartupError::PortInUse { port, .. } => format!("{port} 포트가 사용 중입니다"),
        _ => error.to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            defer,
            link_linear,
            create_todo_from_email,
            pull_linear,
            linear_status,
            set_linear_key,
            open_external,
            server_status,
            gmail_accounts,
            gmail_list,
            gmail_unread_count,
            gmail_get_body,
            gmail_archive,
            gmail_set_read,
            gmail_sync,
            gmail_remove_account,
            gmail_set_credentials,
            gmail_add_account
        ])
        .run(tauri::generate_context!())
        .expect("failed to run todo desktop app");
}
