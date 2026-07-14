use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde::Serialize;
use serde_json::json;
use todo_core::{CreateTodoInput, Status, Todo, TodoCore, TodoFilter, TodoPatch};
use todo_linear::LinearService;

use crate::{
    dto::{
        McpCreateRequest, McpIdRequest, McpLinkRequest, McpListRequest, McpStatusRequest,
        McpUpdateRequest, parse_iso_date, parse_priority, parse_required_status, parse_status,
        parse_todo_id, parse_wire_due_date,
    },
    error::ApiError,
};

#[derive(Clone)]
pub(crate) struct TodoMcp {
    core: TodoCore,
    linear: LinearService,
    tool_router: ToolRouter<Self>,
}

impl TodoMcp {
    pub(crate) fn new(core: TodoCore, linear: LinearService) -> Self {
        Self {
            core,
            linear,
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
        Ok(self
            .core
            .list_todos(TodoFilter {
                status,
                priority,
                due_before,
                query: request.query,
                limit: request.limit,
                offset: None,
            })
            .await?)
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
            || request.status.is_some()
            || request.priority.is_some()
            || request.due_date.is_some();
        if !has_patch {
            return Ok(self.core.get_todo(id).await?);
        }
        let status = parse_status(request.status.as_deref())?;
        let priority = parse_priority(request.priority.as_deref())?;
        Ok(self
            .core
            .update_todo(
                id,
                TodoPatch {
                    title: request.title,
                    description: request.description,
                    status,
                    priority,
                    due_date: request.due_date,
                },
            )
            .await?)
    }
}

#[tool_router]
impl TodoMcp {
    #[tool(
        description = "List active todos. Optional status is todo|in_progress|done; priority is always the string none|urgent|high|medium|low; due_before uses YYYY-MM-DD; query performs full-text search. Deleted todos are excluded. Returns an object with a `todos` array."
    )]
    async fn todo_list(&self, Parameters(request): Parameters<McpListRequest>) -> CallToolResult {
        into_tool_result(
            self.list(request)
                .await
                .map(|todos| json!({ "todos": todos })),
        )
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
        description = "Atomically update any supplied fields of an active todo. status may be todo|in_progress|done and can be combined with other fields. priority must be none|urgent|high|medium|low. due_date accepts tomorrow, fri, 3d, or YYYY-MM-DD; use an empty string or null to clear it."
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
        description = "Link an active todo to a Linear issue identifier such as PI-1234 or a full Linear issue URL. Local title and description are never overwritten."
    )]
    async fn todo_link_linear(
        &self,
        Parameters(request): Parameters<McpLinkRequest>,
    ) -> CallToolResult {
        let result = async {
            let id = parse_todo_id(&request.id)?;
            self.linear.link(id, &request.issue_ref).await?;
            Ok(json!({ "linked": true }))
        }
        .await;
        into_tool_result(result)
    }

    #[tool(
        description = "Import Linear issues assigned to the configured viewer whose workflow state type is started. Existing links are never refreshed. Also closes local linked todos whose remote state is completed or canceled."
    )]
    async fn linear_pull_in_progress(&self) -> CallToolResult {
        into_tool_result(self.linear.pull().await.map_err(ApiError::from))
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
