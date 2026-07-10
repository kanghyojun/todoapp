use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use chrono::{Days, Local};
use reqwest::{Client, Response, StatusCode, header};
use serde_json::{Value, json};
use tempfile::TempDir;
use todo_core::TodoCore;
use todo_server::{ServerConfig, build_router, load_or_create_token};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct TestServer {
    _temp_dir: TempDir,
    base_url: String,
    client: Client,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(dev_origins: Vec<String>) -> Self {
        let temp_dir = tempfile::tempdir().expect("create temporary directory");
        let database = temp_dir.path().join("todo.db");
        let core = TodoCore::connect(&format!("sqlite://{}", database.display()))
            .await
            .expect("connect test database");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("read test address");
        let router = build_router(
            core,
            ServerConfig {
                port: address.port(),
                token: TOKEN.to_owned(),
                dev_origins,
            },
        )
        .expect("build test router");
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve test requests");
        });
        Self {
            _temp_dir: temp_dir,
            base_url: format!("http://{address}"),
            client: Client::new(),
            task,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.bearer_auth(TOKEN)
    }

    async fn create_todo(&self, body: Value) -> Value {
        let response = self
            .authorized(self.client.post(self.url("/api/v1/todos")))
            .json(&body)
            .send()
            .await
            .expect("create todo request");
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.expect("decode created todo")
    }

    async fn mcp(&self, body: Value) -> Response {
        self.authorized(self.client.post(self.url("/mcp")))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-06-18")
            .json(&body)
            .send()
            .await
            .expect("send MCP request")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn authentication_health_host_origin_and_cors_are_enforced() {
    let server = TestServer::start(vec!["http://localhost:2471".to_owned()]).await;

    let health = server
        .client
        .get(server.url("/api/v1/health"))
        .send()
        .await
        .expect("health request");
    assert_eq!(health.status(), StatusCode::OK);

    let missing = server
        .client
        .get(server.url("/api/v1/todos"))
        .send()
        .await
        .expect("missing token request");
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(missing).await, "unauthorized");

    let wrong = server
        .client
        .get(server.url("/api/v1/todos"))
        .bearer_auth("wrong-token")
        .send()
        .await
        .expect("wrong token request");
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let correct = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .send()
        .await
        .expect("correct token request");
    assert_eq!(correct.status(), StatusCode::OK);

    let foreign_origin = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .header(header::ORIGIN, "https://evil.example")
        .send()
        .await
        .expect("foreign origin request");
    assert_eq!(foreign_origin.status(), StatusCode::FORBIDDEN);

    let foreign_host = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .header(header::HOST, "evil.example")
        .send()
        .await
        .expect("foreign host request");
    assert_eq!(foreign_host.status(), StatusCode::FORBIDDEN);

    let preflight = server
        .client
        .request(reqwest::Method::OPTIONS, server.url("/api/v1/todos"))
        .header(header::ORIGIN, "http://localhost:2471")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
        .send()
        .await
        .expect("CORS preflight");
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("CORS origin header"),
        "http://localhost:2471"
    );
}

#[tokio::test]
async fn rest_crud_dates_priority_soft_delete_search_and_linear_stub_work() {
    let server = TestServer::start(Vec::new()).await;
    let expected_tomorrow = Local::now()
        .date_naive()
        .checked_add_days(Days::new(1))
        .expect("tomorrow exists")
        .format("%Y-%m-%d")
        .to_string();
    let created = server
        .create_todo(json!({
            "title": "write integration tests",
            "description": "initial",
            "priority": "urgent",
            "due_date": "tomorrow"
        }))
        .await;
    assert_eq!(created["priority"], "urgent");
    assert!(created["priority"].is_string());
    assert_eq!(created["due_date"], expected_tomorrow);
    let id = created["id"].as_str().expect("created todo id");

    let list: Value = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .send()
        .await
        .expect("list todos")
        .json()
        .await
        .expect("decode todo list");
    assert_eq!(list.as_array().expect("todo array").len(), 1);
    assert!(list[0]["priority"].is_string());

    let fetched: Value = server
        .authorized(
            server
                .client
                .get(server.url(&format!("/api/v1/todos/{id}"))),
        )
        .send()
        .await
        .expect("get todo")
        .json()
        .await
        .expect("decode todo");
    assert_eq!(fetched["id"], id);
    assert!(fetched["priority"].is_string());

    let updated_response = server
        .authorized(
            server
                .client
                .patch(server.url(&format!("/api/v1/todos/{id}"))),
        )
        .json(&json!({
            "description": "updated",
            "priority": "low"
        }))
        .send()
        .await
        .expect("update todo");
    assert_eq!(updated_response.status(), StatusCode::OK);
    let updated: Value = updated_response.json().await.expect("decode updated todo");
    assert_eq!(updated["description"], "updated");
    assert_eq!(updated["priority"], "low");
    assert_eq!(updated["due_date"], expected_tomorrow);

    let status_updated: Value = server
        .authorized(
            server
                .client
                .patch(server.url(&format!("/api/v1/todos/{id}"))),
        )
        .json(&json!({ "title": "x", "status": "done", "due_date": null }))
        .send()
        .await
        .expect("update todo status")
        .json()
        .await
        .expect("decode status update");
    assert_eq!(status_updated["title"], "x");
    assert_eq!(status_updated["status"], "done");
    assert!(status_updated["due_date"].is_null());

    let integer_priority = server
        .authorized(server.client.post(server.url("/api/v1/todos")))
        .json(&json!({ "title": "bad priority", "priority": 2 }))
        .send()
        .await
        .expect("integer priority request");
    assert_eq!(integer_priority.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(integer_priority).await, "invalid_input");

    let invalid_date = server
        .authorized(server.client.post(server.url("/api/v1/todos")))
        .json(&json!({ "title": "bad date", "due_date": "not a date" }))
        .send()
        .await
        .expect("invalid date request");
    assert_eq!(invalid_date.status(), StatusCode::BAD_REQUEST);

    let deleted = server
        .authorized(
            server
                .client
                .delete(server.url(&format!("/api/v1/todos/{id}"))),
        )
        .send()
        .await
        .expect("delete todo");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    for request in [
        server.authorized(
            server
                .client
                .get(server.url(&format!("/api/v1/todos/{id}"))),
        ),
        server
            .authorized(
                server
                    .client
                    .patch(server.url(&format!("/api/v1/todos/{id}"))),
            )
            .json(&json!({ "title": "still deleted" })),
        server
            .authorized(
                server
                    .client
                    .patch(server.url(&format!("/api/v1/todos/{id}"))),
            )
            .json(&json!({ "due_date": "쓰레기" })),
        server
            .authorized(
                server
                    .client
                    .post(server.url(&format!("/api/v1/todos/{id}/link/linear"))),
            )
            .json(&json!({ "issue_ref": "PI-1234" })),
    ] {
        let response = request.send().await.expect("deleted todo request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(error_code(response).await, "not_found");
    }

    let restored: Value = server
        .authorized(
            server
                .client
                .post(server.url(&format!("/api/v1/todos/{id}/restore"))),
        )
        .send()
        .await
        .expect("restore todo")
        .json()
        .await
        .expect("decode restored todo");
    assert_eq!(restored["id"], id);
    assert!(restored["priority"].is_string());

    for query in ["\"", "*", "NOT", "(abc"] {
        let response = server
            .authorized(server.client.get(server.url("/api/v1/todos")))
            .query(&[("q", query)])
            .send()
            .await
            .expect("search request");
        assert_eq!(response.status(), StatusCode::OK, "query {query:?}");
    }

    let link_stub = server
        .authorized(
            server
                .client
                .post(server.url(&format!("/api/v1/todos/{id}/link/linear"))),
        )
        .json(&json!({ "issue_ref": "PI-1234" }))
        .send()
        .await
        .expect("link Linear stub");
    assert_eq!(link_stub.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(error_code(link_stub).await, "not_implemented");

    let pull_stub = server
        .authorized(server.client.post(server.url("/api/v1/linear/pull")))
        .send()
        .await
        .expect("pull Linear stub");
    assert_eq!(pull_stub.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(error_code(pull_stub).await, "not_implemented");
}

#[tokio::test]
async fn rest_combines_search_filters_and_applies_sql_pagination() {
    let server = TestServer::start(Vec::new()).await;
    let deployment_in_progress = server
        .create_todo(json!({
            "title": "배포 진행 작업",
            "status": "in_progress"
        }))
        .await;
    server
        .create_todo(json!({ "title": "배포 대기 작업" }))
        .await;
    server
        .create_todo(json!({
            "title": "문서 진행 작업",
            "status": "in_progress"
        }))
        .await;

    let filtered_response = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .query(&[("q", "배포"), ("status", "in_progress")])
        .send()
        .await
        .expect("combined search request");
    assert_eq!(filtered_response.status(), StatusCode::OK);
    let filtered: Value = filtered_response
        .json()
        .await
        .expect("decode combined search");
    assert_eq!(filtered.as_array().expect("filtered array").len(), 1);
    assert_eq!(filtered[0]["id"], deployment_in_progress["id"]);

    let all: Value = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .send()
        .await
        .expect("full list request")
        .json()
        .await
        .expect("decode full list");
    let page_response = server
        .authorized(server.client.get(server.url("/api/v1/todos")))
        .query(&[("limit", "1"), ("offset", "1")])
        .send()
        .await
        .expect("page request");
    assert_eq!(page_response.status(), StatusCode::OK);
    let page: Value = page_response.json().await.expect("decode page");
    assert_eq!(page.as_array().expect("page array").len(), 1);
    assert_eq!(page[0]["id"], all[1]["id"]);
}

#[tokio::test]
async fn mcp_lists_nine_tools_and_shares_the_core_with_rest() {
    let server = TestServer::start(Vec::new()).await;

    let missing_auth = server
        .client
        .post(server.url("/mcp"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "todo-test", "version": "1" }
            }
        }))
        .send()
        .await
        .expect("unauthenticated MCP request");
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

    let initialized = server
        .mcp(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "todo-test", "version": "1" }
            }
        }))
        .await;
    assert_eq!(initialized.status(), StatusCode::OK);

    let tools_response = server
        .mcp(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }))
        .await;
    assert_eq!(tools_response.status(), StatusCode::OK);
    let tools: Value = tools_response.json().await.expect("decode MCP tools/list");
    let names = tools["result"]["tools"]
        .as_array()
        .expect("MCP tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("MCP tool name"))
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 9);
    for expected in [
        "todo_list",
        "todo_get",
        "todo_create",
        "todo_update",
        "todo_set_status",
        "todo_delete",
        "todo_restore",
        "todo_link_linear",
        "linear_pull_in_progress",
    ] {
        assert!(names.contains(&expected), "missing MCP tool {expected}");
    }

    let create_response = server
        .mcp(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "todo_create",
                "arguments": {
                    "title": "created through MCP",
                    "priority": "high",
                    "due_date": "3d"
                }
            }
        }))
        .await;
    assert_eq!(create_response.status(), StatusCode::OK);
    let create: Value = create_response.json().await.expect("decode MCP create");
    assert_eq!(create["result"]["isError"], false);
    assert_eq!(create["result"]["structuredContent"]["priority"], "high");
    assert!(create["result"]["structuredContent"]["priority"].is_string());
    let id = create["result"]["structuredContent"]["id"]
        .as_str()
        .expect("MCP-created todo id");

    let update_response = server
        .mcp(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "todo_update",
                "arguments": {
                    "id": id,
                    "title": "updated through MCP",
                    "status": "done",
                    "due_date": null
                }
            }
        }))
        .await;
    let update: Value = update_response.json().await.expect("decode MCP update");
    assert_eq!(update["result"]["isError"], false);
    assert_eq!(
        update["result"]["structuredContent"]["title"],
        "updated through MCP"
    );
    assert_eq!(update["result"]["structuredContent"]["status"], "done");
    assert!(update["result"]["structuredContent"]["due_date"].is_null());

    let rest_todo: Value = server
        .authorized(
            server
                .client
                .get(server.url(&format!("/api/v1/todos/{id}"))),
        )
        .send()
        .await
        .expect("REST get after MCP create")
        .json()
        .await
        .expect("decode REST todo");
    assert_eq!(rest_todo["title"], "updated through MCP");
    assert_eq!(rest_todo["priority"], "high");
    assert_eq!(rest_todo["status"], "done");

    let stub_response = server
        .mcp(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": { "name": "linear_pull_in_progress", "arguments": {} }
        }))
        .await;
    let stub: Value = stub_response.json().await.expect("decode MCP stub error");
    assert_eq!(stub["result"]["isError"], true);
    assert!(
        stub["result"]["content"][0]["text"]
            .as_str()
            .expect("MCP error text")
            .contains("not_implemented")
    );
}

#[test]
fn token_is_stable_hex_and_stored_with_private_permissions() {
    let temp_dir = tempfile::tempdir().expect("create token tempdir");
    let path = temp_dir.path().join("config/todo/token");
    let first = load_or_create_token(&path).expect("create token");
    let second = load_or_create_token(&path).expect("reload token");
    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn standalone_binary_fails_loudly_when_the_requested_port_is_occupied() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy a local port");
    let port = occupied.local_addr().expect("occupied address").port();
    let temp_dir = tempfile::tempdir().expect("binary tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_todo-server"))
        .arg("--port")
        .arg(port.to_string())
        .arg("--database")
        .arg(temp_dir.path().join("todo.db"))
        .env("HOME", temp_dir.path())
        .output()
        .expect("run todo-server binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains(&format!("port {port} on 127.0.0.1 is already in use")));
}

async fn error_code(response: Response) -> String {
    response
        .json::<Value>()
        .await
        .expect("decode error response")["error"]["code"]
        .as_str()
        .expect("error code")
        .to_owned()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("read token metadata")
        .permissions()
        .mode()
        & 0o777
}
