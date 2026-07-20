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
use todo_core::{Todo, TodoCore, TodoFilterInput};
use todo_linear::{LinearService, LinearStatus, PendingChoice, PullSummary};

use crate::{
    dto::{
        CreateTodoRequest, DeferRequest, LinearDoneStateRequest, LinearKeyRequest,
        LinearLinkRequest, UpdateTodoRequest, parse_json, parse_query, resolve_ref,
    },
    error::ApiError,
};

#[derive(Clone)]
struct AppState {
    core: TodoCore,
    linear: LinearService,
}

pub(crate) fn router(core: TodoCore, linear: LinearService) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/todos", get(list_todos).post(create_todo))
        .route(
            "/api/v1/todos/{id}",
            get(get_todo).patch(update_todo).delete(delete_todo),
        )
        .route("/api/v1/todos/{id}/restore", post(restore_todo))
        .route("/api/v1/todos/{id}/defer", post(defer_todo))
        .route("/api/v1/todos/{id}/link/linear", post(link_linear))
        .route("/api/v1/linear/pull", post(linear_pull))
        .route(
            "/api/v1/linear/key",
            post(set_linear_key).delete(delete_linear_key),
        )
        .route("/api/v1/linear/status", get(linear_status))
        .route("/api/v1/linear/pending-choices", get(pending_choices))
        .route("/api/v1/linear/done-state", post(set_done_state))
        .route("/api/v1/linear/retry", post(retry_linear))
        .with_state(AppState { core, linear })
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn list_todos(
    State(state): State<AppState>,
    query: Result<Query<TodoFilterInput>, QueryRejection>,
) -> Result<Json<Vec<Todo>>, ApiError> {
    let query = parse_query(query)?;
    Ok(Json(state.core.list_todos(query.into_filter()?).await?))
}

async fn create_todo(
    State(state): State<AppState>,
    payload: Result<Json<CreateTodoRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Todo>), ApiError> {
    let todo = state
        .core
        .create_todo(parse_json(payload)?.into_core()?)
        .await?;
    Ok((StatusCode::CREATED, Json(todo)))
}

async fn get_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    Ok(Json(
        state
            .core
            .get_todo(resolve_ref(&state.core, &id).await?)
            .await?,
    ))
}

async fn update_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateTodoRequest>, JsonRejection>,
) -> Result<Json<Todo>, ApiError> {
    let id = resolve_ref(&state.core, &id).await?;
    let request = parse_json(payload)?;
    Ok(Json(
        state
            .core
            .update_todo(id, request.into_core_patch())
            .await?,
    ))
}

async fn delete_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .core
        .delete_todo(resolve_ref(&state.core, &id).await?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    Ok(Json(
        state
            .core
            .restore_todo(resolve_ref(&state.core, &id).await?)
            .await?,
    ))
}

async fn defer_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Result<Json<DeferRequest>, JsonRejection>,
) -> Result<Json<Todo>, ApiError> {
    let id = resolve_ref(&state.core, &id).await?;
    // 본문이 비어도 무기한 보류로 받는다.
    let request = payload.map(|Json(request)| request).unwrap_or_default();
    Ok(Json(state.core.defer_todo(id, &request.until).await?))
}

async fn link_linear(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Result<Json<LinearLinkRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve_ref(&state.core, &id).await?;
    let payload = parse_json(payload)?;
    state.linear.link(id, &payload.issue_ref).await?;
    Ok(Json(json!({ "linked": true })))
}

async fn linear_pull(State(state): State<AppState>) -> Result<Json<PullSummary>, ApiError> {
    Ok(Json(state.linear.pull().await?))
}

async fn set_linear_key(
    State(state): State<AppState>,
    payload: Result<Json<LinearKeyRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    state
        .linear
        .set_api_key(&parse_json(payload)?.api_key)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_linear_key(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.linear.delete_api_key().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn linear_status(State(state): State<AppState>) -> Result<Json<LinearStatus>, ApiError> {
    Ok(Json(state.linear.status().await?))
}

async fn pending_choices(
    State(state): State<AppState>,
) -> Result<Json<Vec<PendingChoice>>, ApiError> {
    Ok(Json(state.linear.pending_choices().await?))
}

async fn set_done_state(
    State(state): State<AppState>,
    payload: Result<Json<LinearDoneStateRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let request = parse_json(payload)?;
    state
        .linear
        .set_done_state(&request.team_id, &request.state_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn retry_linear(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let retried = state.linear.retry_failing().await?;
    Ok(Json(json!({ "retried": retried })))
}
