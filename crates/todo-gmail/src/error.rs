use thiserror::Error;

#[derive(Debug, Error)]
#[error("token store operation failed")]
pub struct TokenStoreError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Gmail is not configured")]
    NotConfigured,
    #[error("Google rejected the credentials")]
    Unauthorized,
    #[error("account was not found")]
    AccountNotFound,
    #[error("message was not found")]
    MessageNotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("history is too old to replay")]
    HistoryExpired,
    #[error("{message}")]
    Remote { message: String, retryable: bool },
    #[error("database error: {0}")]
    Database(String),
    #[error(transparent)]
    TokenStore(#[from] TokenStoreError),
}

impl Error {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Remote {
                retryable: true,
                ..
            }
        )
    }

    pub(crate) fn safe_message(&self) -> String {
        match self {
            Self::Remote { message, .. } => message.clone(),
            other => other.to_string(),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<todo_core::Error> for Error {
    fn from(error: todo_core::Error) -> Self {
        Self::Database(error.to_string())
    }
}
