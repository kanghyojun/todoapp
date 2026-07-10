use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use todo_core::{Todo, TodoCore};

use crate::{
    dto::{
        CreateTodoRequest, LinearLinkRequest, ListQuery, UpdateTodoRequest, parse_json,
        parse_query, parse_todo_id, validate_issue_ref,
    },
    error::ApiError,
};

pub(crate) fn router(core: TodoCore) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/todos", get(list_todos).post(create_todo))
        .route(
            "/api/v1/todos/{id}",
            get(get_todo).patch(update_todo).delete(delete_todo),
        )
        .route("/api/v1/todos/{id}/restore", post(restore_todo))
        .route("/api/v1/todos/{id}/link/linear", post(link_linear))
        .route("/api/v1/linear/pull", post(linear_pull))
        .with_state(core)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn list_todos(
    State(core): State<TodoCore>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Vec<Todo>>, ApiError> {
    let query = parse_query(query)?;
    let mut todos = if let Some(search) = query.q.as_deref() {
        if query.has_filters() {
            return Err(ApiError::invalid_input(
                "q cannot currently be combined with status, priority, or due_before",
            ));
        }
        core.search_todos(search).await?
    } else {
        core.list_todos(query.core_filter()?).await?
    };
    todos = paginate(todos, query.offset, query.limit);
    Ok(Json(todos))
}

async fn create_todo(
    State(core): State<TodoCore>,
    payload: Result<Json<CreateTodoRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Todo>), ApiError> {
    let todo = core.create_todo(parse_json(payload)?.into_core()?).await?;
    Ok((StatusCode::CREATED, Json(todo)))
}

async fn get_todo(
    State(core): State<TodoCore>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    Ok(Json(core.get_todo(parse_todo_id(&id)?).await?))
}

async fn update_todo(
    State(core): State<TodoCore>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateTodoRequest>, JsonRejection>,
) -> Result<Json<Todo>, ApiError> {
    let id = parse_todo_id(&id)?;
    let request = parse_json(payload)?;
    let status = request.status;
    let has_patch = request.has_todo_patch();
    if status.is_some() && has_patch {
        return Err(ApiError::invalid_input(
            "status cannot currently be changed in the same request as other fields",
        ));
    }
    let todo = match status {
        Some(status) => core.set_status(id, status).await?,
        None if has_patch => core.update_todo(id, request.into_core_patch()?).await?,
        None => core.get_todo(id).await?,
    };
    Ok(Json(todo))
}

async fn delete_todo(
    State(core): State<TodoCore>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    core.delete_todo(parse_todo_id(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_todo(
    State(core): State<TodoCore>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    Ok(Json(core.restore_todo(parse_todo_id(&id)?).await?))
}

async fn link_linear(
    State(core): State<TodoCore>,
    Path(id): Path<String>,
    payload: Result<Json<LinearLinkRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_todo_id(&id)?;
    let payload = parse_json(payload)?;
    core.get_todo(id).await?;
    validate_issue_ref(&payload.issue_ref)?;
    Err(ApiError::not_implemented(
        "linking Linear issues is not implemented until M5",
    ))
}

async fn linear_pull() -> Result<Json<Value>, ApiError> {
    Err(ApiError::not_implemented(
        "pulling Linear issues is not implemented until M5",
    ))
}

fn paginate<T>(items: Vec<T>, offset: usize, limit: Option<usize>) -> Vec<T> {
    let remaining = items.into_iter().skip(offset);
    match limit {
        Some(limit) => remaining.take(limit).collect(),
        None => remaining.collect(),
    }
}
