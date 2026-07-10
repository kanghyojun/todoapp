use axum::extract::rejection::{JsonRejection, QueryRejection};
use chrono::{Local, NaiveDate};
use rmcp::schemars;
use serde::{Deserialize, Deserializer};
use todo_core::{CreateTodoInput, Priority, Status, TodoFilter, TodoPatch, parse_due_date};

use crate::error::ApiError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateTodoRequest {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) status: Option<Status>,
    #[serde(default)]
    pub(crate) priority: Priority,
    pub(crate) due_date: Option<String>,
}

impl CreateTodoRequest {
    pub(crate) fn into_core(self) -> Result<CreateTodoInput, ApiError> {
        Ok(CreateTodoInput {
            title: self.title,
            description: self.description,
            status: self.status.unwrap_or(Status::Todo),
            priority: self.priority,
            due_date: parse_wire_due_date(self.due_date.as_deref())?.flatten(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateTodoRequest {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<Status>,
    pub(crate) priority: Option<Priority>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub(crate) due_date: Option<Option<String>>,
}

impl UpdateTodoRequest {
    pub(crate) fn into_core_patch(self) -> TodoPatch {
        TodoPatch {
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            due_date: self.due_date,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    pub(crate) status: Option<Status>,
    pub(crate) priority: Option<Priority>,
    pub(crate) due_before: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
}

impl ListQuery {
    pub(crate) fn into_core_filter(self) -> Result<TodoFilter, ApiError> {
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
pub(crate) struct LinearLinkRequest {
    pub(crate) issue_ref: String,
}

pub(crate) fn parse_json<T>(value: Result<axum::Json<T>, JsonRejection>) -> Result<T, ApiError> {
    value
        .map(|axum::Json(payload)| payload)
        .map_err(|error| ApiError::invalid_input(error.body_text()))
}

pub(crate) fn parse_query<T>(
    value: Result<axum::extract::Query<T>, QueryRejection>,
) -> Result<T, ApiError> {
    value
        .map(|axum::extract::Query(query)| query)
        .map_err(|error| ApiError::invalid_input(error.body_text()))
}

pub(crate) fn parse_todo_id(value: &str) -> Result<todo_core::TodoId, ApiError> {
    value
        .parse()
        .map_err(|_| ApiError::invalid_input("id must be a UUID"))
}

pub(crate) fn parse_iso_date(value: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| ApiError::invalid_input("date must use YYYY-MM-DD format"))
}

pub(crate) fn parse_wire_due_date(
    value: Option<&str>,
) -> Result<Option<Option<NaiveDate>>, ApiError> {
    value
        .map(|input| {
            parse_due_date(input, Local::now().date_naive())
                .map_err(|error| ApiError::invalid_input(error.to_string()))
        })
        .transpose()
}

fn deserialize_optional_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub(crate) fn parse_priority(value: Option<&str>) -> Result<Option<Priority>, ApiError> {
    value.map(parse_required_priority).transpose()
}

fn parse_required_priority(value: &str) -> Result<Priority, ApiError> {
    match value {
        "none" => Ok(Priority::None),
        "urgent" => Ok(Priority::Urgent),
        "high" => Ok(Priority::High),
        "medium" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        _ => Err(ApiError::invalid_input(
            "priority must be one of none, urgent, high, medium, low",
        )),
    }
}

pub(crate) fn parse_status(value: Option<&str>) -> Result<Option<Status>, ApiError> {
    value.map(parse_required_status).transpose()
}

pub(crate) fn parse_required_status(value: &str) -> Result<Status, ApiError> {
    match value {
        "todo" => Ok(Status::Todo),
        "in_progress" => Ok(Status::InProgress),
        "done" => Ok(Status::Done),
        _ => Err(ApiError::invalid_input(
            "status must be one of todo, in_progress, done",
        )),
    }
}

pub(crate) fn validate_issue_ref(value: &str) -> Result<(), ApiError> {
    let Some((team, number)) = value.split_once('-') else {
        return Err(ApiError::invalid_input("issue_ref must look like PI-1234"));
    };
    let valid_team = !team.is_empty()
        && team
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
    let valid_number =
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) && number != "0";
    if !valid_team || !valid_number || value.matches('-').count() != 1 {
        return Err(ApiError::invalid_input("issue_ref must look like PI-1234"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpListRequest {
    #[schemars(description = "Optional status: todo, in_progress, or done")]
    pub(crate) status: Option<String>,
    #[schemars(description = "Optional priority: none, urgent, high, medium, or low")]
    pub(crate) priority: Option<String>,
    #[schemars(description = "Only return todos due before this YYYY-MM-DD date")]
    pub(crate) due_before: Option<String>,
    #[schemars(description = "Optional full-text search query")]
    pub(crate) query: Option<String>,
    #[schemars(description = "Maximum number of todos to return")]
    pub(crate) limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpIdRequest {
    #[schemars(description = "Todo UUID")]
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpCreateRequest {
    #[schemars(description = "Required todo title")]
    pub(crate) title: String,
    #[schemars(description = "Optional details; defaults to an empty string")]
    pub(crate) description: Option<String>,
    #[schemars(description = "Priority string: none, urgent, high, medium, or low")]
    pub(crate) priority: Option<String>,
    #[schemars(description = "Natural-language due date such as tomorrow, fri, 3d, or 2026-04-20")]
    pub(crate) due_date: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpUpdateRequest {
    #[schemars(description = "Todo UUID")]
    pub(crate) id: String,
    #[schemars(description = "New title; omitted leaves it unchanged")]
    pub(crate) title: Option<String>,
    #[schemars(description = "New details; omitted leaves them unchanged")]
    pub(crate) description: Option<String>,
    #[schemars(description = "New status: todo, in_progress, or done")]
    pub(crate) status: Option<String>,
    #[schemars(description = "Priority string: none, urgent, high, medium, or low")]
    pub(crate) priority: Option<String>,
    #[schemars(description = "Natural-language due date; use an empty string to clear it")]
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub(crate) due_date: Option<Option<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpStatusRequest {
    #[schemars(description = "Todo UUID")]
    pub(crate) id: String,
    #[schemars(description = "New status: todo, in_progress, or done")]
    pub(crate) status: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct McpLinkRequest {
    #[schemars(description = "Todo UUID")]
    pub(crate) id: String,
    #[schemars(description = "Linear issue reference such as PI-1234")]
    pub(crate) issue_ref: String,
}
