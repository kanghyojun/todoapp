use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use clap::Parser;
use todo_core::TodoCore;
use todo_linear::{LinearService, SystemKeyStore};
use todo_server::{
    ServerConfig, StartupError, bind, build_router_with_linear, default_token_path,
    load_or_create_token, serve,
};

#[derive(Debug, Parser)]
#[command(
    name = "todo-server",
    about = "Serve the todo REST API and MCP endpoint"
)]
struct Args {
    #[arg(long, default_value_t = 2470)]
    port: u16,
    #[arg(long, default_value = "~/.local/share/todo/todo.db")]
    database: PathBuf,
    #[arg(long = "dev-origin")]
    dev_origins: Vec<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("todo-server failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), StartupError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(StartupError::HomeNotSet)?;
    let database = absolute_path(expand_home(&args.database, &home))?;
    if let Some(parent) = database.parent() {
        fs::create_dir_all(parent).map_err(|source| StartupError::File {
            operation: "create database directory",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let database_url = format!("sqlite://{}", database.display());
    let core = TodoCore::connect(&database_url).await?;
    let token = load_or_create_token(&default_token_path(&home))?;
    let listener = bind(args.port).await?;
    let linear = LinearService::new(core.clone(), Arc::new(SystemKeyStore));
    let router = build_router_with_linear(
        core,
        linear.clone(),
        ServerConfig {
            port: args.port,
            token,
            dev_origins: args.dev_origins,
        },
    )?;
    // 라우터를 만드는 일과 워커를 띄우는 일은 다르다. 호출자가 정한다.
    linear.spawn_worker();
    let address = listener.local_addr().map_err(|source| StartupError::Bind {
        port: args.port,
        source,
    })?;
    eprintln!("todo-server listening on http://{address}");
    serve(listener, router).await
}

fn expand_home(path: &Path, home: &Path) -> PathBuf {
    if path == Path::new("~") {
        return home.to_path_buf();
    }
    match path.strip_prefix("~/") {
        Ok(relative) => home.join(relative),
        Err(_) => path.to_path_buf(),
    }
}

fn absolute_path(path: PathBuf) -> Result<PathBuf, StartupError> {
    if path.is_absolute() {
        return Ok(path);
    }
    env::current_dir()
        .map(|current| current.join(&path))
        .map_err(|source| StartupError::File {
            operation: "resolve database path",
            path,
            source,
        })
}
