use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use todo_core::Error as CoreError;
use todo_linear::Error as LinearError;

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_input",
            message: message.into(),
        }
    }

    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "a valid bearer token is required".to_owned(),
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "unauthorized",
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }

    pub(crate) fn into_tool_message(self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl From<LinearError> for ApiError {
    fn from(error: LinearError) -> Self {
        match error {
            LinearError::NotConfigured => Self {
                status: StatusCode::CONFLICT,
                code: "linear_not_configured",
                message: "Linear API key is not configured".to_owned(),
            },
            LinearError::Unauthorized => Self {
                status: StatusCode::UNAUTHORIZED,
                code: "linear_unauthorized",
                message: "Linear rejected the API key".to_owned(),
            },
            LinearError::IssueNotFound => Self {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
                message: "Linear issue not found".to_owned(),
            },
            LinearError::InvalidInput(message) => Self::invalid_input(message),
            LinearError::Core(error) => error.into(),
            LinearError::Remote { .. } => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "linear_api_error",
                message: "the Linear API request failed".to_owned(),
            },
            LinearError::Database(detail) => {
                eprintln!("todo-server Linear database error: {detail}");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "internal_error",
                    message: "the database operation failed".to_owned(),
                }
            }
            LinearError::KeyStore(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "key_store_error",
                message: "the OS keychain is unavailable".to_owned(),
            },
        }
    }
}

impl From<CoreError> for ApiError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::NotFound(_) => Self {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
                message: "todo not found".to_owned(),
            },
            CoreError::InvalidInput(message) => Self::invalid_input(message),
            CoreError::IssueAlreadyLinked => Self {
                status: StatusCode::CONFLICT,
                code: "issue_already_linked",
                message: "the Linear issue is already linked".to_owned(),
            },
            CoreError::ParseDueDate(error) => Self::invalid_input(error.to_string()),
            CoreError::Database(detail) => {
                eprintln!("todo-server database error: {detail}");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "internal_error",
                    message: "the database operation failed".to_owned(),
                }
            }
        }
    }
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}
