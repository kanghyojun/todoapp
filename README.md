# todo

키보드만으로 굴리는 개인용 할 일 앱. 데이터는 내 기기의 SQLite에 있고, 다른 프로그램이 붙도록 REST와 MCP를 엽니다. Linear 이슈와 느슨하게 이어집니다.

설계는 [기획서](docs/superpowers/specs/2026-07-10-todo-app-design.md)에 있습니다. 코드보다 그쪽이 먼저입니다.

## 구조

```
crates/todo-core     도메인, SQLite, 유스케이스, 이벤트    ← 규칙이 전부 여기 있다
crates/todo-linear   Linear GraphQL, 아웃박스 워커
crates/todo-server   axum. /api/v1/* 와 /mcp 를 한 포트(2470)에서
src/                 Solid + Vite
src-tauri/           Tauri 셸 (별도 workspace)
```

`todo-core`는 tauri도 axum도 rmcp도 모릅니다. 세 어댑터가 각자 코어를 부릅니다. 로직이 핸들러로 새면 설계 위반입니다.

## 개발

### 리눅스 (GUI 없는 개발 서버)

`src-tauri`는 여기서 빌드되지 않습니다. `webkit2gtk`가 필요합니다. 나머지는 전부 됩니다.

```bash
# 1) 서버를 띄운다
cargo run -p todo-server -- --dev-origin http://localhost:2471

# 2) 다른 셸에서 UI 를 띄운다
export VITE_TODO_TOKEN=$(cat ~/.config/todo/token)
pnpm install --reporter=append-only
pnpm dev            # http://localhost:2471
```

브라우저로 열려면 SSH 포트 포워딩을 씁니다. `ssh -L 2471:localhost:2471 <host>`

UI는 `TodoClient` 인터페이스를 거칩니다. 브라우저에서는 `HttpClient`가 REST로, Tauri 앱에서는 `TauriClient`가 IPC로 붙습니다. 그래서 화면 없는 기기에서도 UI를 만들 수 있습니다.

### macOS (실제로 쓰는 곳)

```bash
pnpm install --frozen-lockfile --reporter=append-only
pnpm tauri dev      # 개발
pnpm tauri build    # 배포용 .app
```

Xcode Command Line Tools가 필요합니다. `xcode-select --install`

## 테스트

```bash
cargo test --workspace                        # 49개. src-tauri 는 workspace 밖이다
cargo clippy --all-targets -- -D warnings
cargo fmt --check
pnpm exec tsc --noEmit
pnpm exec vitest run                          # 29개
```

`src/client.e2e.test.ts`는 진짜 서버에 대고 도는 계약 검증입니다. mock fetch로는 wire 불일치를 못 잡습니다. 서버를 띄우고 환경변수를 주면 돕니다.

```bash
cargo run -p todo-server -- --port 2477 &
TODO_E2E_TOKEN=$(cat ~/.config/todo/token) \
TODO_E2E_BASE=http://127.0.0.1:2477/api/v1 \
  pnpm exec vitest run
```

## MCP 등록

앱이나 서버가 떠 있어야 합니다.

```bash
claude mcp add --transport http todo http://127.0.0.1:2470/mcp \
  --header "Authorization: Bearer $(cat ~/.config/todo/token)"
```

도구 아홉 개를 노출합니다. `todo_list`, `todo_get`, `todo_create`, `todo_update`, `todo_set_status`, `todo_delete`, `todo_restore`, `todo_link_linear`, `linear_pull_in_progress`.

### 다른 기기에서 붙기 (Tailscale)

기본은 `127.0.0.1` 바인딩이라 같은 기기에서만 붙습니다. Tailscale로 연결된 리눅스 서버 같은 다른 기기에서 붙이려면 바인드 주소를 바꿉니다. 토큰과 같은 폴더의 `~/.config/todo/config.json`:

```json
{ "bind": "100.92.89.75" }
```

앱을 다시 켜면 그 주소에 바인딩되고, `<주소>:2470`이 Host 화이트리스트에 자동으로 들어갑니다. 그다음 리눅스에서 등록합니다.

```bash
claude mcp add --transport http todo http://100.92.89.75:2470/mcp \
  --header "Authorization: Bearer <맥의 ~/.config/todo/token 값>"
```

`todo-server` 바이너리는 `--bind 100.92.89.75` 플래그로 같은 일을 합니다. 바인드는 Tailscale IP처럼 특정 인터페이스로 좁히십시오. `0.0.0.0`은 LAN 전체에 노출됩니다.

우선순위는 언제나 문자열입니다. `none`, `urgent`, `high`, `medium`, `low`. 마감일은 쓰기에서 자연어를 받습니다. `tomorrow`, `fri`, `3d`, `2026-04-20`, 그리고 빈 문자열은 지우기입니다.

## 보안

REST와 MCP는 `127.0.0.1`에만 바인딩됩니다. 그것만으로는 부족합니다. 아무 웹페이지나 `fetch('http://127.0.0.1:2470/...')`를 던질 수 있습니다. 세 겹으로 막습니다.

1. `127.0.0.1` 바인딩(기본). `config.json`으로 넓히면 그 주소만 Host 화이트리스트에 더해집니다.
2. `~/.config/todo/token`(0600)의 Bearer 토큰. `GET /api/v1/health`만 예외입니다. 비교는 상수 시간입니다.
3. `Host`와 `Origin` 검증

Linear API 키는 OS 키체인에 있습니다. SQLite에도 파일에도 없습니다.

배포된 Tauri 앱의 웹뷰는 IPC를 쓰므로 토큰을 갖지 않습니다. 브라우저에 토큰이 노출되는 건 개발 전용 경로입니다.

## Linear

동작이 셋뿐입니다. 동기화 엔진이 아닙니다.

**링크** `l`을 눌러 `PI-1234`나 URL을 넣으면 이슈를 한 번 조회해 연결합니다. 로컬 제목과 설명은 건드리지 않습니다.

**가져오기** 팔레트에서 수동 실행합니다. 내게 할당된 In Progress 이슈 중 **링크가 없는 것만** 새 할 일로 만듭니다. 이미 링크된 건 제목이 바뀌었어도 그대로 둡니다. 지운 할 일은 되살아나지 않습니다. 같은 실행에서 역방향 한 번, Linear가 끝낸 이슈는 로컬도 done으로 내립니다.

**밀어넣기** done을 누르면 상태 변경과 같은 트랜잭션에서 아웃박스에 쌓이고, 워커가 재시도하며 밀어넣습니다. 비행기 안에서 눌러도 잃어버리지 않습니다. done을 취소해도 아무것도 보내지 않습니다.

팀의 완료 상태가 여럿이면(Done, Merged 등) 추측하지 않고 한 번 물어본 뒤 기억합니다.

## 아직 없는 것

전역 quick capture, 캘린더·메일 탭, 기기 간 동기화. 스키마와 단축키 스코프에 자리만 비워뒀습니다.
