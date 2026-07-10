use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde::Serialize;
use serde_json::{Value, json};
use todo_core::{CreateTodoInput, Status, Todo, TodoCore, TodoFilter, TodoPatch};

use crate::{
    dto::{
        McpCreateRequest, McpIdRequest, McpLinkRequest, McpListRequest, McpStatusRequest,
        McpUpdateRequest, parse_iso_date, parse_priority, parse_required_status, parse_status,
        parse_todo_id, parse_wire_due_date, validate_issue_ref,
    },
    error::ApiError,
};

#[derive(Clone)]
pub(crate) struct TodoMcp {
    core: TodoCore,
    tool_router: ToolRouter<Self>,
}

impl TodoMcp {
    pub(crate) fn new(core: TodoCore) -> Self {
        Self {
            core,
            tool_router: Self::tool_router(),
        }
    }

    async fn list(&self, request: McpListRequest) -> Result<Vec<Todo>, ApiError> {
        let status = parse_status(request.status.as_deref())?;
        let priority = parse_priority(request.priority.as_deref())?;
        let due_before = request
            .due_before
            .as_deref()
            .map(parse_iso_date)
            .transpose()?;
        let mut todos = if let Some(query) = request.query.as_deref() {
            if status.is_some() || priority.is_some() || due_before.is_some() {
                return Err(ApiError::invalid_input(
                    "query cannot currently be combined with status, priority, or due_before",
                ));
            }
            self.core.search_todos(query).await?
        } else {
            self.core
                .list_todos(TodoFilter {
                    status,
                    priority,
                    due_before,
                })
                .await?
        };
        if let Some(limit) = request.limit {
            todos.truncate(limit);
        }
        Ok(todos)
    }

    async fn create(&self, request: McpCreateRequest) -> Result<Todo, ApiError> {
        let priority = parse_priority(request.priority.as_deref())?.unwrap_or_default();
        let due_date = parse_wire_due_date(request.due_date.as_deref())?.flatten();
        Ok(self
            .core
            .create_todo(CreateTodoInput {
                title: request.title,
                description: request.description.unwrap_or_default(),
                status: Status::Todo,
                priority,
                due_date,
            })
            .await?)
    }

    async fn update(&self, request: McpUpdateRequest) -> Result<Todo, ApiError> {
        let id = parse_todo_id(&request.id)?;
        let has_patch = request.title.is_some()
            || request.description.is_some()
            || request.priority.is_some()
            || request.due_date.is_some();
        if !has_patch {
            return Ok(self.core.get_todo(id).await?);
        }
        let priority = parse_priority(request.priority.as_deref())?;
        let due_date = parse_wire_due_date(request.due_date.as_deref())?;
        Ok(self
            .core
            .update_todo(
                id,
                TodoPatch {
                    title: request.title,
                    description: request.description,
                    priority,
                    due_date,
                },
            )
            .await?)
    }
}

#[tool_router]
impl TodoMcp {
    #[tool(
        description = "List active todos. Optional status is todo|in_progress|done; priority is always the string none|urgent|high|medium|low; due_before uses YYYY-MM-DD; query performs full-text search. Deleted todos are excluded."
    )]
    async fn todo_list(&self, Parameters(request): Parameters<McpListRequest>) -> CallToolResult {
        into_tool_result(self.list(request).await)
    }

    #[tool(
        description = "Get one active todo by UUID. The response priority is the string none|urgent|high|medium|low. A soft-deleted or unknown todo returns an error."
    )]
    async fn todo_get(&self, Parameters(request): Parameters<McpIdRequest>) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            Ok(self.core.get_todo(id).await?)
        }
        .await;
        into_tool_result(result)
    }

    #[tool(
        description = "Create a todo. title is required. priority must be the string none|urgent|high|medium|low. due_date accepts natural language such as tomorrow, fri, 3d, or 2026-04-20; an unparseable date is an error."
    )]
    async fn todo_create(
        &self,
        Parameters(request): Parameters<McpCreateRequest>,
    ) -> CallToolResult {
        into_tool_result(self.create(request).await)
    }

    #[tool(
        description = "Update only the supplied fields of an active todo. priority must be none|urgent|high|medium|low. due_date accepts tomorrow, fri, 3d, or YYYY-MM-DD; use an empty string to clear it. Use todo_set_status for status changes."
    )]
    async fn todo_update(
        &self,
        Parameters(request): Parameters<McpUpdateRequest>,
    ) -> CallToolResult {
        into_tool_result(self.update(request).await)
    }

    #[tool(
        description = "Set an active todo's status to todo, in_progress, or done. A soft-deleted or unknown todo returns an error."
    )]
    async fn todo_set_status(
        &self,
        Parameters(request): Parameters<McpStatusRequest>,
    ) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            let status = parse_required_status(&request.status)?;
            Ok(self.core.set_status(id, status).await?)
        }
        .await;
        into_tool_result(result)
    }

    #[tool(
        description = "Soft-delete an active todo by UUID. It disappears from get, list, update, and link operations but can be recovered with todo_restore."
    )]
    async fn todo_delete(&self, Parameters(request): Parameters<McpIdRequest>) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            self.core.delete_todo(id).await?;
            Ok(json!({ "id": id, "deleted": true }))
        }
        .await;
        into_tool_result(result)
    }

    #[tool(description = "Restore a soft-deleted todo by UUID and return it.")]
    async fn todo_restore(&self, Parameters(request): Parameters<McpIdRequest>) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            Ok(self.core.restore_todo(id).await?)
        }
        .await;
        into_tool_result(result)
    }

    #[tool(
        description = "Link an active todo to a Linear issue reference such as PI-1234. This is intentionally unavailable until M5 and currently returns a not_implemented tool error; it never fabricates a Linear issue ID."
    )]
    async fn todo_link_linear(
        &self,
        Parameters(request): Parameters<McpLinkRequest>,
    ) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            self.core.get_todo(id).await?;
            validate_issue_ref(&request.issue_ref)?;
            Err::<Value, _>(ApiError::not_implemented(
                "linking Linear issues is not implemented until M5",
            ))
        }
        .await;
        into_tool_result(result)
    }

    #[tool(
        description = "Pull in-progress Linear issues into the local todo database. This integration is intentionally unavailable until M5 and currently returns a not_implemented tool error."
    )]
    async fn linear_pull_in_progress(&self) -> CallToolResult {
        into_tool_result(Err::<Value, _>(ApiError::not_implemented(
            "pulling Linear issues is not implemented until M5",
        )))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TodoMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Manage local todos. Priority values are strings: none, urgent, high, medium, or low. Due dates accept natural language on writes.",
            );
        // 기본값은 rmcp 자신의 이름과 버전이다. MCP 클라이언트 목록에 라이브러리 이름이 뜬다.
        info.server_info.name = "todo".to_owned();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_owned();
        info
    }
}

fn into_tool_result<T: Serialize>(result: Result<T, ApiError>) -> CallToolResult {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => CallToolResult::structured(value),
            Err(_) => CallToolResult::error(vec![ContentBlock::text(
                "internal_error: failed to encode the tool result",
            )]),
        },
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.into_tool_message())]),
    }
}
