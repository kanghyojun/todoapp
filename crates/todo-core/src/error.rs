use thiserror::Error;

use crate::{ParseDueDateError, TodoId};

#[derive(Debug, Error)]
pub enum Error {
    #[error("todo not found: {0}")]
    NotFound(TodoId),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("Linear issue is already linked")]
    IssueAlreadyLinked,
    #[error("database error: {0}")]
    Database(String),
    #[error(transparent)]
    ParseDueDate(#[from] ParseDueDateError),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<sqlx::migrate::MigrateError> for Error {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        Self::Database(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
