mod dto;
mod error;
mod mcp;
mod rest;
mod security;

use std::{io, path::PathBuf};

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    middleware,
};
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use thiserror::Error;
use todo_core::{Error as CoreError, TodoCore};
use todo_linear::LinearService;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub use security::{default_token_path, load_or_create_token};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub token: String,
    pub dev_origins: Vec<String>,
}

#[derive(Debug, Error)]
pub enum StartupError {
    #[error("cannot {operation} at {path}: {source}")]
    File {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("token file at {0} must contain exactly 64 hexadecimal characters")]
    InvalidTokenFile(PathBuf),
    #[error("invalid development origin: {0}")]
    InvalidOrigin(String),
    #[error("failed to initialize the todo database: {0}")]
    Database(#[from] CoreError),
    #[error("port {port} on 127.0.0.1 is already in use")]
    PortInUse { port: u16 },
    #[error("cannot bind 127.0.0.1:{port}: {source}")]
    Bind {
        port: u16,
        #[source]
        source: io::Error,
    },
    #[error("todo server stopped with an I/O error: {0}")]
    Serve(#[source] io::Error),
    #[error("HOME is not set, so default todo paths cannot be resolved")]
    HomeNotSet,
}

pub fn build_router_with_linear(
    core: TodoCore,
    linear: LinearService,
    config: ServerConfig,
) -> Result<Router, StartupError> {
    let allowed_hosts = vec![
        format!("127.0.0.1:{}", config.port),
        format!("localhost:{}", config.port),
    ];
    let mut allowed_origins = vec![
        format!("http://127.0.0.1:{}", config.port),
        format!("http://localhost:{}", config.port),
    ];
    for origin in &config.dev_origins {
        validate_origin(origin)?;
        if !allowed_origins.contains(origin) {
            allowed_origins.push(origin.clone());
        }
    }

    let mcp_config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(allowed_hosts.clone())
        .with_allowed_origins(allowed_origins.clone());
    let mcp_core = core.clone();
    let mcp_linear = linear.clone();
    let mcp_service: StreamableHttpService<mcp::TodoMcp, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(mcp::TodoMcp::new(mcp_core.clone(), mcp_linear.clone())),
            Default::default(),
            mcp_config,
        );

    let security = security::SecurityState::new(config.token, allowed_hosts, allowed_origins);
    let mut router = rest::router(core, linear.clone())
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(
            security.clone(),
            security::authenticate,
        ));

    if !config.dev_origins.is_empty() {
        let origins = config
            .dev_origins
            .iter()
            .map(|origin| {
                HeaderValue::from_str(origin)
                    .map_err(|_| StartupError::InvalidOrigin(origin.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        router = router.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        );
    }

    Ok(router.layer(middleware::from_fn_with_state(
        security,
        security::validate_host_and_origin,
    )))
}

pub async fn bind(port: u16) -> Result<tokio::net::TcpListener, StartupError> {
    match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => Ok(listener),
        Err(source) if source.kind() == io::ErrorKind::AddrInUse => {
            Err(StartupError::PortInUse { port })
        }
        Err(source) => Err(StartupError::Bind { port, source }),
    }
}

pub async fn serve(listener: tokio::net::TcpListener, router: Router) -> Result<(), StartupError> {
    axum::serve(listener, router)
        .await
        .map_err(StartupError::Serve)
}

fn validate_origin(origin: &str) -> Result<(), StartupError> {
    let valid_scheme = origin.starts_with("http://") || origin.starts_with("https://");
    let no_path = origin
        .split_once("://")
        .is_some_and(|(_, authority)| !authority.is_empty() && !authority.contains('/'));
    if !valid_scheme || !no_path {
        return Err(StartupError::InvalidOrigin(origin.to_owned()));
    }
    Ok(())
}
