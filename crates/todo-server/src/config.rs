use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

use serde::Deserialize;

const DEFAULT_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// 설정 파일 경로. 토큰(`~/.config/todo/token`)과 같은 폴더에 둬서 함께 관리한다.
pub fn default_config_path(home: &Path) -> PathBuf {
    home.join(".config/todo/config.json")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    bind: Option<String>,
}

/// 설정 파일에서 서버 바인드 주소를 읽는다. 파일이 없거나, 비었거나, 깨졌거나,
/// bind 값이 IP 로 파싱되지 않으면 루프백으로 떨어진다. 앱이 안 뜨는 것보다
/// 로컬로라도 뜨는 게 낫다. 잘못된 값은 로그로 남겨 조용한 실패를 피한다.
pub fn read_bind_config(path: &Path) -> IpAddr {
    let Ok(text) = fs::read_to_string(path) else {
        return DEFAULT_BIND; // 파일 없음: 로컬 전용, 정상.
    };
    if text.trim().is_empty() {
        return DEFAULT_BIND;
    }
    let config: FileConfig = match serde_json::from_str(&text) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "todo config: {} 파싱 실패({error}), 127.0.0.1 로 바인딩합니다",
                path.display()
            );
            return DEFAULT_BIND;
        }
    };
    match config.bind {
        Some(raw) => raw.parse().unwrap_or_else(|_| {
            eprintln!("todo config: '{raw}' 는 올바른 IP 가 아니라 127.0.0.1 로 바인딩합니다");
            DEFAULT_BIND
        }),
        None => DEFAULT_BIND,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use tempfile::tempdir;

    use super::read_bind_config;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    fn config_with(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().expect("create config tempdir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, contents).expect("write config");
        (dir, path)
    }

    #[test]
    fn missing_file_falls_back_to_loopback() {
        let dir = tempdir().expect("create config tempdir");
        let path = dir.path().join("does-not-exist.json");
        assert_eq!(read_bind_config(&path), LOOPBACK);
    }

    #[test]
    fn empty_file_falls_back_to_loopback() {
        let (_dir, path) = config_with("");
        assert_eq!(read_bind_config(&path), LOOPBACK);
    }

    #[test]
    fn config_without_bind_falls_back_to_loopback() {
        let (_dir, path) = config_with("{}");
        assert_eq!(read_bind_config(&path), LOOPBACK);
    }

    #[test]
    fn valid_bind_is_used() {
        let (_dir, path) = config_with(r#"{ "bind": "100.92.89.75" }"#);
        assert_eq!(
            read_bind_config(&path),
            IpAddr::V4(Ipv4Addr::new(100, 92, 89, 75))
        );
    }

    #[test]
    fn unparseable_bind_falls_back_to_loopback() {
        let (_dir, path) = config_with(r#"{ "bind": "not-an-ip" }"#);
        assert_eq!(read_bind_config(&path), LOOPBACK);
    }
}
