use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{Request, State},
    http::{Method, header},
    middleware::Next,
    response::Response,
};
use rand::Rng as _;
use subtle::ConstantTimeEq as _;

use crate::{StartupError, error::ApiError};

#[derive(Clone)]
pub(crate) struct SecurityState {
    token: Arc<[u8]>,
    allowed_hosts: Arc<[String]>,
    allowed_origins: Arc<[String]>,
}

impl SecurityState {
    pub(crate) fn new(
        token: String,
        allowed_hosts: Vec<String>,
        allowed_origins: Vec<String>,
    ) -> Self {
        Self {
            token: Arc::from(token.into_bytes()),
            allowed_hosts: Arc::from(allowed_hosts),
            allowed_origins: Arc::from(allowed_origins),
        }
    }
}

pub(crate) async fn authenticate(
    State(state): State<SecurityState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.method() == Method::GET && request.uri().path() == "/api/v1/health" {
        return Ok(next.run(request).await);
    }

    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "));
    let authorized = supplied
        .map(|value| bool::from(state.token.as_ref().ct_eq(value)))
        .unwrap_or(false);
    if !authorized {
        return Err(ApiError::unauthorized());
    }
    Ok(next.run(request).await)
}

pub(crate) async fn validate_host_and_origin(
    State(state): State<SecurityState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("Host header is required"))?;
    if !state
        .allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return Err(ApiError::forbidden("Host header is not allowed"));
    }

    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let origin = origin
            .to_str()
            .map_err(|_| ApiError::forbidden("Origin header is invalid"))?;
        if !state
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
        {
            return Err(ApiError::forbidden("Origin header is not allowed"));
        }
    }
    Ok(next.run(request).await)
}

pub fn load_or_create_token(path: &Path) -> Result<String, StartupError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StartupError::File {
            operation: "create token directory",
            path: parent.to_path_buf(),
            source,
        })?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
            StartupError::File {
                operation: "secure token directory",
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            let mut random = [0_u8; 32];
            rand::rng().fill(&mut random);
            let token = encode_hex(&random);
            file.write_all(token.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|source| StartupError::File {
                    operation: "write token",
                    path: path.to_path_buf(),
                    source,
                })?;
            Ok(token)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => read_existing_token(path),
        Err(source) => Err(StartupError::File {
            operation: "create token",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_existing_token(path: &Path) -> Result<String, StartupError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        StartupError::File {
            operation: "secure token",
            path: path.to_path_buf(),
            source,
        }
    })?;
    let mut token = String::new();
    OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|mut file| file.read_to_string(&mut token))
        .map_err(|source| StartupError::File {
            operation: "read token",
            path: path.to_path_buf(),
            source,
        })?;
    let token = token.trim().to_owned();
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StartupError::InvalidTokenFile(path.to_path_buf()));
    }
    Ok(token)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

pub fn default_token_path(home: &Path) -> PathBuf {
    home.join(".config/todo/token")
}
