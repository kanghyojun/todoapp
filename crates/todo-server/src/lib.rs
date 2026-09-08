mod config;
mod dto;
mod error;
mod mcp;
mod rest;
mod security;

use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
};

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

pub use config::{default_config_path, read_bind_config};
pub use security::{default_token_path, load_or_create_token};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: IpAddr,
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
    #[error("port {port} on {bind} is already in use")]
    PortInUse { bind: IpAddr, port: u16 },
    #[error("cannot bind {bind}:{port}: {source}")]
    Bind {
        bind: IpAddr,
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
    let allowed_hosts = allowed_hosts_for(config.bind, config.port);
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

pub async fn bind(bind: IpAddr, port: u16) -> Result<tokio::net::TcpListener, StartupError> {
    match tokio::net::TcpListener::bind((bind, port)).await {
        Ok(listener) => Ok(listener),
        Err(source) if source.kind() == io::ErrorKind::AddrInUse => {
            Err(StartupError::PortInUse { bind, port })
        }
        Err(source) => Err(StartupError::Bind { bind, port, source }),
    }
}

/// 루프백은 반드시 연다(실패하면 에러). 추가 주소(예: Tailscale IP)가 루프백이
/// 아니면 best-effort 로 함께 연다. 그 바인딩이 실패하면(예: Tailscale off) 경고만
/// 남기고 루프백 리스너로 계속 굴린다. 로컬을 원격 상태에 묶지 않기 위해서다.
pub async fn bind_local_and(
    extra: IpAddr,
    port: u16,
) -> Result<Vec<tokio::net::TcpListener>, StartupError> {
    let mut listeners = vec![bind(IpAddr::V4(Ipv4Addr::LOCALHOST), port).await?];
    if !extra.is_loopback() {
        match bind(extra, port).await {
            Ok(listener) => listeners.push(listener),
            Err(error) => {
                eprintln!("todo: {extra}:{port} 바인딩 실패({error}), 루프백만 엽니다");
            }
        }
    }
    Ok(listeners)
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

/// 로컬 루프백은 언제나 허용한다. 바인드 주소가 루프백이 아니면(예: Tailscale IP)
/// 그 `<주소>:<포트>`도 Host 화이트리스트에 넣는다. 그래야 원격에서 붙을 수 있다.
fn allowed_hosts_for(bind: IpAddr, port: u16) -> Vec<String> {
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !bind.is_loopback() {
        hosts.push(format!("{bind}:{port}"));
    }
    hosts
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{allowed_hosts_for, bind_local_and};

    #[test]
    fn loopback_bind_allows_only_local_hosts() {
        let hosts = allowed_hosts_for(IpAddr::V4(Ipv4Addr::LOCALHOST), 2470);
        assert_eq!(hosts, vec!["127.0.0.1:2470", "localhost:2470"]);
    }

    #[test]
    fn non_loopback_bind_also_allows_its_own_host() {
        let bind = IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1));
        let hosts = allowed_hosts_for(bind, 2470);
        assert!(hosts.contains(&"127.0.0.1:2470".to_owned()));
        assert!(hosts.contains(&"localhost:2470".to_owned()));
        assert!(hosts.contains(&"100.64.0.1:2470".to_owned()));
    }

    #[tokio::test]
    async fn bind_local_and_opens_only_loopback_for_loopback_extra() {
        let listeners = bind_local_and(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
            .await
            .expect("loopback must bind");
        assert_eq!(listeners.len(), 1);
        assert!(listeners[0].local_addr().expect("addr").ip().is_loopback());
    }

    #[tokio::test]
    async fn bind_local_and_keeps_loopback_when_extra_is_unreachable() {
        // 192.0.2.0/24 는 TEST-NET-1. 이 기기에 없으니 추가 바인딩은 실패한다.
        let extra: IpAddr = "192.0.2.1".parse().expect("test-net address");
        let listeners = bind_local_and(extra, 0)
            .await
            .expect("loopback still binds");
        assert_eq!(listeners.len(), 1);
        assert!(listeners[0].local_addr().expect("addr").ip().is_loopback());
    }
}
