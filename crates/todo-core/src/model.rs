use std::{fmt, str::FromStr};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TodoId(Uuid);

impl TodoId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TodoId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TodoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TodoId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    Done,
}

impl Status {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, Error> {
        match value {
            "todo" => Ok(Self::Todo),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            _ => Err(Error::InvalidInput(format!(
                "unknown status stored in database: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    None,
    Urgent,
    High,
    Medium,
    Low,
}

impl Priority {
    pub(crate) const fn as_db_integer(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Urgent => 1,
            Self::High => 2,
            Self::Medium => 3,
            Self::Low => 4,
        }
    }

    pub(crate) fn from_db(value: i64) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Urgent),
            2 => Ok(Self::High),
            3 => Ok(Self::Medium),
            4 => Ok(Self::Low),
            _ => Err(Error::InvalidInput(format!(
                "unknown priority stored in database: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    pub due_date: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTodoInput {
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    pub due_date: Option<NaiveDate>,
}

impl Default for CreateTodoInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::None,
            due_date: None,
        }
    }
}

impl CreateTodoInput {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodoPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub due_date: Option<Option<NaiveDate>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodoFilter {
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub due_before: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearLinkInput {
    pub issue_id: String,
    pub identifier: String,
    pub url: String,
    pub team_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainEvent {
    TodoCreated(TodoId),
    TodoUpdated(TodoId),
    TodoDeleted(TodoId),
    TodoRestored(TodoId),
    SyncStateChanged,
}
