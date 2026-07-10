# 로컬 우선 Todo 앱 설계

작성일: 2026-07-10
작성자: 강효준 (kanghyojun)
상태: 승인됨, 구현 계획 대기

## 1. 무엇을 만드는가

키보드만으로 굴러가는 개인용 할 일 앱입니다. 데스크톱 앱으로 돌고, 데이터는 내 기기의 SQLite에 있고, 다른 프로그램이 붙을 수 있도록 REST와 MCP 인터페이스를 엽니다. 내가 쓰는 Linear 이슈와 느슨하게 이어집니다.

벤치마크는 Superhuman입니다. 정확히는 Superhuman의 기능이 아니라 **손이 자판을 안 떠나는 감각**이 목표입니다. 마우스를 잡는 순간 이 앱은 실패한 것으로 봅니다.

### 목표

- 할 일 하나를 만들고, 우선순위와 마감일을 정하고, 상태를 옮기는 데 손이 홈 포지션을 떠나지 않는다.
- Claude Code 같은 도구가 내 할 일을 읽고 쓸 수 있다.
- Linear 이슈를 로컬로 끌어와 작업하고, 여기서 끝내면 Linear도 끝난다.

### 비목표

이번 범위에서 다루지 않습니다. 나중에 필요해지면 그때 별도 설계를 씁니다.

- 여러 기기 동기화. 당분간 한 기기만 씁니다. 다만 스키마는 나중에 갈아엎지 않아도 되도록 만듭니다.
- 협업, 공유, 다중 사용자.
- 앱 밖에서 부르는 전역 핫키(quick capture). 2차로 미룹니다.
- 캘린더·메일 탭. 4절에서 확장 지점만 비워둡니다.
- 하위 할 일, 반복 할 일, 태그, 프로젝트. 필요해진 적이 없습니다.

## 2. 결정된 전제

설계의 근거입니다. 이 전제가 흔들리면 아래 설계도 다시 봐야 합니다.

| 항목 | 결정 |
|---|---|
| 할 일의 원본 | 로컬이 원본이다. Linear는 링크된 것만 미러한다. |
| 인터페이스 범위 | 내 머신 안에서만 쓴다. 127.0.0.1 바인딩. |
| 기기 | 당분간 한 기기. |
| 미래 탭 | 캘린더·메일은 완전히 별도 화면. 데이터가 섞이지 않는다. |
| 단축키 모델 | Superhuman 스타일. 단일키 + 커맨드 팔레트. |
| Linear 링크 | 이슈 ID나 URL을 붙여넣어 연결한다. |
| Linear 상태 | 단방향. 로컬 done만 Linear로 민다. done 취소는 안 민다. |
| Linear 가져오기 | 동기화가 아니라 가져오기(import)다. 내게 할당된 In Progress 이슈를 읽어 로컬 todo를 만들고 링크를 건다. 두 번째 실행에서 기존 항목은 건드리지 않는다. |
| 프론트엔드 | Solid + TypeScript + Vite |
| 우선순위 표기 | Urgent / High / Medium / Low / None |

가져오기는 sync가 아닙니다. 제목과 설명을 채워 넣고 링크를 걸어두는 정도입니다. 그 뒤로 그 todo는 내 것입니다. 유일한 예외가 하나 있습니다. Linear에서 이미 끝난(completed 또는 canceled) 이슈가 로컬에 살아있으면, 가져오기를 누를 때 로컬도 done으로 내립니다. 이미 끝낸 일을 두 번 지우는 수고를 없애기 위해서입니다.

## 3. 아키텍처

세 개의 인터페이스(앱 UI, REST, MCP)가 같은 데이터를 만집니다. 그러니 로직은 한 곳에만 있어야 합니다.

```
todo/
├─ crates/
│  ├─ todo-core/     도메인, SQLite, 유스케이스, 이벤트 버스
│  ├─ todo-linear/   Linear GraphQL 클라이언트, 아웃박스 워커
│  └─ todo-server/   axum. /api/v1/* 와 /mcp
├─ src-tauri/        Tauri 셸. 코어 초기화, IPC command, 서버 spawn, 이벤트 emit
└─ src/              Solid + Vite
```

`todo-core`는 tauri도, axum도, rmcp도 모릅니다. 세 어댑터가 각자 코어를 부릅니다. MCP 도구는 REST를 거치지 않고 코어를 직접 부릅니다. 자기 자신에게 HTTP를 왕복시킬 이유가 없습니다.

각 크레이트의 경계입니다.

**todo-core** — 하는 일: 할 일의 생성·수정·상태 전이·삭제·복구·검색, SQLite 영속화, 도메인 이벤트 발행, 자연어 날짜 파싱, 아웃박스 적재. 의존: sqlx, chrono, uuid, tokio(broadcast). 이 크레이트만 알면 이 앱의 규칙을 전부 아는 셈이어야 합니다.

**todo-linear** — 하는 일: Linear GraphQL 호출, API 키를 OS 키체인에서 읽기, 아웃박스를 훑어 재시도. 의존: todo-core(타입과 저장소), reqwest, keyring.

**todo-server** — 하는 일: HTTP 라우팅, 토큰 인증, DTO 변환, MCP 도구 등록. 의존: todo-core, axum, rmcp. 로직이 여기 들어오면 설계 위반입니다.

**src-tauri** — 하는 일: 앱 부팅, 코어 초기화, IPC command 노출, 서버를 백그라운드 태스크로 띄우기, 도메인 이벤트를 웹뷰로 emit.

### 프로세스와 이벤트

REST와 MCP 서버는 Tauri 앱 프로세스 안에서 돕니다. 앱을 끄면 같이 꺼집니다. 이 선택 덕에 SQLite 쓰기 경합이 없고, 데몬 관리(자동 시작, 재기동, 좀비 프로세스)를 안 만들어도 됩니다.

MCP로 todo가 바뀌었을 때 화면이 갱신되어야 하니, 코어가 `tokio::sync::broadcast`로 도메인 이벤트를 쏩니다.

```rust
enum DomainEvent {
    TodoCreated(TodoId),
    TodoUpdated(TodoId),
    TodoDeleted(TodoId),
    TodoRestored(TodoId),
    SyncStateChanged,
}
```

`src-tauri`가 이 채널을 구독해 `app.emit("todo:changed", ..)`으로 웹뷰에 밀어줍니다. 프론트는 이벤트를 받으면 해당 쿼리를 다시 읽습니다. 팔레트에서 만든 todo든 Claude Code가 만든 todo든 화면 갱신 경로가 같습니다.

앱 UI는 HTTP가 아니라 Tauri IPC로 코어를 부릅니다. 서버가 안 떠도 앱은 정상 동작합니다.

### 프론트엔드의 클라이언트 어댑터

프론트는 Tauri IPC를 직접 부르지 않습니다. 얇은 인터페이스 하나를 거칩니다.

```ts
interface TodoClient {
  list(filter: Filter): Promise<Todo[]>
  get(id: string): Promise<Todo>
  create(input: NewTodo): Promise<Todo>
  update(id: string, patch: TodoPatch): Promise<Todo>
  setStatus(id: string, status: Status): Promise<void>
  remove(id: string): Promise<void>
  restore(id: string): Promise<void>
  linkLinear(id: string, issueRef: string): Promise<void>
  pullLinear(): Promise<PullResult>
  subscribe(onChange: () => void): () => void
}
```

구현이 둘입니다. `TauriClient`는 IPC로, `HttpClient`는 REST로 붙습니다. 배포된 앱은 `TauriClient`를 씁니다. HTTP 왕복이 없다는 원래의 이점이 그대로 유지됩니다.

`HttpClient`는 개발용입니다. 개발 기기가 GUI 없는 리눅스 서버이므로, `pnpm dev`로 Vite를 띄우고 브라우저에서 UI와 단축키를 다듬습니다. 이때 데이터는 REST로 옵니다. 가상 디스플레이나 VNC가 필요 없습니다.

부수 효과가 둘입니다. REST를 매일 쓰게 되므로 품질이 저절로 검증됩니다. 그리고 Tauri 셸이 얇아집니다. 창을 만들고 IPC를 배선하는 것 외에 할 일이 없습니다.

`subscribe`는 변경 알림입니다. `TauriClient`는 `todo:changed` 이벤트를, `HttpClient`는 폴링을 씁니다. 개발용이므로 폴링으로 충분합니다.

### 포트와 실패

포트는 2470으로 고정합니다. MCP 클라이언트 설정 파일에 박히는 값이라 매번 바뀌면 안 됩니다. 포트가 이미 점유돼 있으면 조용히 다른 포트로 옮기지 않습니다. 앱은 그대로 뜨고, 상단에 "외부 인터페이스를 열지 못했습니다. 2470 포트가 사용 중입니다"라는 배너를 띄웁니다. UI는 IPC를 쓰므로 아무 지장이 없습니다.

### 보안

localhost 바인딩만으로는 부족합니다. 브라우저에서 아무 웹페이지나 `http://127.0.0.1:2470`으로 요청을 던질 수 있습니다. 세 겹으로 막습니다.

1. `127.0.0.1`에만 바인딩합니다. `0.0.0.0`은 쓰지 않습니다.
2. 최초 실행 시 32바이트 랜덤 토큰을 만들어 `~/.config/todo/token`에 0600 권한으로 저장합니다. REST와 MCP 모두 `Authorization: Bearer <token>`을 요구합니다. 토큰이 없거나 틀리면 401입니다. 유일한 예외는 `GET /api/v1/health`입니다.
3. rmcp의 `StreamableHttpServerConfig`에서 `with_allowed_hosts`와 `with_allowed_origins`를 켜서 Host·Origin을 검증합니다. REST 라우터에도 같은 검사를 미들웨어로 겁니다.

Linear API 키는 SQLite에 넣지 않습니다. `keyring` 크레이트로 OS 키체인에 저장합니다.

## 4. 데이터 모델

SQLite를 씁니다. 단일 기기, 로컬, 앱 내장이라는 조건에서 이보다 나은 선택이 없습니다. 다만 그냥 쓰지 않고 네 가지를 얹습니다. WAL 모드, 아웃박스 테이블, FTS5 인덱스, 그리고 동기화 친화적 기본기(UUIDv7 기본키, `updated_at`, 소프트 삭제)입니다.

셋째와 넷째는 지금 당장 필요하지 않지만, 나중에 붙이려면 스키마를 갈아엎어야 하는 것들입니다. 지금 넣는 비용이 거의 0입니다.

접속 시 실행할 pragma입니다.

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
```

### todos

```sql
CREATE TABLE todos (
    id            TEXT PRIMARY KEY,          -- UUIDv7. 시간순 정렬 가능
    title         TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'todo'
                  CHECK (status IN ('todo', 'in_progress', 'done')),
    priority      INTEGER NOT NULL DEFAULT 0
                  CHECK (priority BETWEEN 0 AND 4),
    due_date      TEXT,                      -- 'YYYY-MM-DD'. 시각은 없다
    completed_at  TEXT,                      -- RFC3339 UTC
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    deleted_at    TEXT,                      -- 소프트 삭제

    priority_rank INTEGER GENERATED ALWAYS AS
                  (CASE priority WHEN 0 THEN 5 ELSE priority END) VIRTUAL
);

CREATE INDEX idx_todos_active   ON todos(status, priority_rank, due_date)
                                WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_due      ON todos(due_date) WHERE deleted_at IS NULL;
CREATE INDEX idx_todos_updated  ON todos(updated_at);
```

우선순위는 Linear의 규약을 그대로 씁니다. `0=None, 1=Urgent, 2=High, 3=Medium, 4=Low`. 연동할 때 변환 함수가 필요 없어집니다.

그런데 이대로 `ORDER BY priority`를 하면 None이 맨 앞에 옵니다. 그래서 `priority_rank` 생성 컬럼을 두고 None만 5로 밀어냅니다. 인덱스는 이 컬럼에 겁니다.

`todos`는 rowid 테이블입니다. `WITHOUT ROWID`를 쓰지 않습니다. FTS5 external content가 rowid를 요구하기 때문입니다.

### linear_links

```sql
CREATE TABLE linear_links (
    todo_id            TEXT PRIMARY KEY REFERENCES todos(id) ON DELETE CASCADE,
    issue_id           TEXT NOT NULL UNIQUE,   -- Linear 내부 UUID
    identifier         TEXT NOT NULL,          -- 'PI-1234'
    url                TEXT NOT NULL,
    team_id            TEXT NOT NULL,
    last_pushed_status TEXT,                   -- 마지막으로 Linear에 민 상태
    linked_at          TEXT NOT NULL
);
```

`issue_id`가 UNIQUE라서 한 이슈가 두 todo에 링크되는 일이 없습니다. 가져오기가 멱등해지는 근거이기도 합니다.

todo를 소프트 삭제해도 링크는 남습니다. 그래서 가져오기를 다시 눌러도 지워진 todo가 되살아나지 않습니다. `issue_id`가 이미 `linear_links`에 있으면 건너뜁니다.

### sync_outbox

```sql
CREATE TABLE sync_outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    todo_id         TEXT NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,             -- 'linear_complete'
    payload         TEXT NOT NULL DEFAULT '{}',
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    last_error      TEXT,
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE UNIQUE INDEX idx_outbox_pending
    ON sync_outbox(todo_id, kind) WHERE completed_at IS NULL;

CREATE INDEX idx_outbox_ready
    ON sync_outbox(next_attempt_at) WHERE completed_at IS NULL;
```

이 테이블이 설계에서 제일 값어치를 합니다. done을 눌렀을 때 Linear API를 그 자리에서 부르지 않습니다. 한 트랜잭션 안에서 todo 상태와 아웃박스 항목을 함께 커밋하고, 워커가 나중에 처리합니다.

얻는 것이 셋입니다. UI가 네트워크를 기다리며 멈추지 않습니다. 비행기 안에서 done을 눌러도 잃어버리지 않습니다. 그리고 부분 실패(로컬은 done인데 Linear는 아님)가 원천적으로 생기지 않습니다.

부분 유니크 인덱스가 멱등성을 보장합니다. 같은 todo에 대해 처리되지 않은 `linear_complete`가 이미 있으면 두 번 쌓이지 않습니다.

### settings

```sql
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

쓰이는 키입니다.

- `linear.done_state.<team_id>` — 그 팀에서 done으로 밀 때 쓸 워크플로우 상태 ID
- `linear.viewer_id` — 내 Linear 사용자 ID. 가져오기 필터에 씁니다
- `server.token_created_at`

### todos_fts

```sql
CREATE VIRTUAL TABLE todos_fts USING fts5(
    title, description,
    content = 'todos',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);
```

`todos`의 INSERT·UPDATE·DELETE에 트리거를 걸어 인덱스를 맞춥니다. 검색과 커맨드 팔레트가 여기 붙습니다.

한국어는 unicode61 토크나이저로 어절 단위까지만 잘립니다. 형태소 분석은 하지 않습니다. 실사용에서 "배포"로 "배포 스크립트"를 찾는 데는 충분하고, 접두 검색(`배포*`)까지 붙이면 체감상 부족함이 없습니다. 부족해지면 그때 trigram 토크나이저를 검토합니다.

### 마이그레이션

`sqlx migrate`를 씁니다. 마이그레이션 파일은 `crates/todo-core/migrations/`에 두고, 앱 시작 시 자동 적용합니다.

### SQLite 말고 다른 선택지는?

검토했고 전부 접었습니다.

`redb`나 `sled` 같은 임베디드 KV는 쓰기가 빠르지만 "마감일 임박순, 우선순위순" 같은 질의를 손으로 짜야 합니다. 전문 검색도 직접 만들어야 합니다. 얻는 게 없습니다.

JSON 파일이나 마크다운은 검색과 정렬에서 무너집니다.

`libSQL`은 나중에 여러 기기를 붙일 때 embedded replica가 매력적입니다. 다만 SQL 방언이 SQLite와 호환되므로, 그때 갈아타면 됩니다. 지금 앞당겨 도입할 이유가 없습니다.

## 5. 도메인 규칙

### 상태

`todo`, `in_progress`, `done` 셋뿐입니다. 전이에 제약이 없습니다. 어디서 어디로든 갑니다.

`done`으로 갈 때 `completed_at`을 찍고, `done`에서 나올 때 지웁니다. `done`으로 갔고 그 todo에 Linear 링크가 있으면 같은 트랜잭션에서 아웃박스에 `linear_complete`를 넣습니다.

### 삭제와 복구

삭제는 `deleted_at`을 찍는 것입니다. 확인 모달을 띄우지 않습니다. 대신 "삭제됨. u로 되돌리기" 토스트를 띄웁니다. 되돌리기가 있으면 확인이 필요 없고, 확인이 없어야 한 타로 끝납니다.

코어는 `delete_todo(id)`와 `restore_todo(id)`를 함께 제공합니다.

목록과 검색은 `deleted_at IS NULL`인 것만 봅니다. 영구 삭제는 만들지 않습니다.

### 되돌리기(undo)

프론트가 인메모리 스택(최근 20개)에 역동작을 쌓습니다. 코어에 명령 로그를 두지 않습니다.

- done 토글 → 이전 상태로 `set_status`
- 우선순위 변경 → 이전 우선순위로 `update`
- 삭제 → `restore_todo`
- 생성 → `delete_todo`

되돌리기는 이 앱 UI에서 내가 한 동작만 대상으로 합니다. MCP나 REST로 일어난 변경은 되돌리기 대상이 아닙니다. 그게 자연스럽습니다.

### 마감일

날짜만 다룹니다. 시각은 없습니다. `YYYY-MM-DD` 문자열로 저장하고, 로컬 타임존 기준으로 해석합니다.

자연어 파싱은 `todo-core`에 둡니다. 그래야 MCP와 REST에서도 `"tomorrow"`를 쓸 수 있습니다.

받아들이는 입력: `today`, `tomorrow`, `tmr`, `mon`~`sun`, `next week`(다음 주 월요일), `3d`·`2w`(오늘부터 N일/N주 뒤), `4/20`(올해. 이미 지났으면 내년), `2026-04-20`, 그리고 빈 문자열(마감일 제거).

요일은 "오늘 다음에 오는 그 요일"입니다. 오늘이 수요일일 때 `fri`는 이틀 뒤, `mon`은 닷새 뒤입니다. 오늘과 같은 요일을 넣으면(수요일에 `wed`) 오늘이 아니라 다음 주 수요일입니다. 오늘을 뜻하려면 `today`가 있으니, 굳이 요일로 오늘을 가리킬 이유가 없습니다.

파싱에 실패하면 저장하지 않고 오류를 돌려줍니다. 조용히 오늘로 넣는 식의 추측을 하지 않습니다.

### 정렬

기본 정렬입니다.

1. `in_progress`가 맨 위. 그다음 `todo`. `done`은 맨 아래.
2. 마감일 임박순. 마감일이 없는 것은 있는 것보다 뒤.
3. `priority_rank` 오름차순 (Urgent가 먼저).
4. `created_at` 오름차순.

## 6. 사용자 인터페이스

### 화면 구조

상단에 탭 바가 있습니다. 지금은 Todo 탭 하나뿐입니다. 그 아래 필터 칩(전체 / todo / in progress / done), 그 아래 목록입니다. 상세는 목록 오른쪽에 붙는 패널로 엽니다.

목록은 TanStack Virtual 계열의 가상화 없이 시작합니다. 개인용이라 수천 건이 되기 전에는 필요 없습니다. 프로파일링으로 느려지는 게 확인되면 그때 넣습니다.

### 단축키

| 키 | 동작 |
|---|---|
| `j` `k` | 아래·위 이동 |
| `Enter` | 상세 열기 |
| `Esc` | 상세 닫기 / 편집 취소 / 선택 해제 |
| `c` | 새 todo. 인라인 입력. Enter로 저장, Shift+Enter로 저장 후 연속 입력 |
| `e` | 제목 인라인 편집 |
| `d` | done 토글 |
| `i` | in progress 토글 |
| `p` 다음 `u` `h` `m` `l` `n` | Urgent / High / Medium / Low / None |
| `t` | 마감일 입력 (자연어) |
| `x` | 선택 토글 |
| `Backspace` | 삭제 (확인 없음. 소프트 삭제) |
| `l` | Linear 이슈 링크 |
| `o` | 링크된 Linear 이슈를 브라우저로 열기 |
| `1` `2` `3` `0` | 필터: todo / in progress / done / 전체 |
| `/` | 검색 |
| `u` | 되돌리기 |
| `⌘K` (Linux·Windows에서 `Ctrl+K`) | 커맨드 팔레트 |
| `?` | 단축키 도움말 |

`x`로 여러 개를 고른 뒤 `d`, `p`, `t`를 누르면 일괄 적용됩니다.

`p` 다음 `u` 같은 두 타 조합이 있으니 입력기는 짧은 대기 상태를 갖습니다. 500ms 안에 두 번째 키가 안 오면 조합을 취소하고 아무 일도 하지 않습니다.

입력 필드에 포커스가 있으면 단일키 단축키는 죽습니다. `Esc`와 `Enter`만 삽니다.

### 단축키 시스템의 구조

전역 스코프와 뷰 스코프로 나눕니다. `⌘K`, `?`, `/`는 전역입니다. `j`, `k`, `d`, `i`, `p`, `t`는 Todo 뷰가 소유합니다.

이렇게 나누는 이유는 나중에 캘린더 탭이 붙을 때입니다. 캘린더 탭에서 `d`는 다른 뜻이거나 아무 뜻이 없어야 합니다. 지금 스코프를 나눠두지 않으면 그때 전부 뜯어야 합니다.

키 처리기는 순수 함수로 뽑습니다. `(현재 상태, 키 이벤트) -> 액션 | 없음`. 이래야 테스트할 수 있습니다.

### 커맨드 팔레트

`⌘K`로 엽니다. 목록은 두 종류가 섞입니다. 실행할 수 있는 명령(새 todo, Linear 가져오기, 설정 열기, 단축키 도움말)과 할 일 검색 결과입니다. 검색은 FTS5로 갑니다.

명령을 고르면 실행하고, 할 일을 고르면 목록에서 그 항목으로 커서를 옮깁니다.

## 7. Linear 연동

동작이 셋뿐입니다. 링크, 가져오기, 밀어넣기.

### 인증

Linear Personal API key를 설정 화면에서 한 번 입력받아 OS 키체인에 넣습니다. 키가 없으면 Linear 관련 명령이 팔레트에 나타나지 않고, `l`을 눌러도 "Linear API 키를 먼저 설정하십시오"만 뜹니다.

키를 넣을 때 `viewer { id }`를 한 번 불러 유효성을 확인하고, 결과를 `settings.linear.viewer_id`에 저장합니다.

### 링크

`l`을 누르면 입력창이 뜹니다. `PI-1234` 형태의 identifier나 이슈 URL을 받습니다.

GraphQL로 이슈를 한 번 조회해서 `issue_id`, `identifier`, `url`, `team_id`를 `linear_links`에 넣습니다. **로컬 todo의 제목과 설명은 건드리지 않습니다.** 이미 내가 쓴 것이 있기 때문입니다.

이슈를 못 찾으면 오류를 띄웁니다. 다른 todo가 이미 그 이슈에 링크돼 있으면(`issue_id` UNIQUE 위반) "이 이슈는 이미 '...'에 연결돼 있습니다"라고 알려줍니다.

### 가져오기

팔레트에서 `Pull Linear issues`를 실행합니다. 자동으로 돌지 않습니다.

내게 할당됐고 워크플로우 상태 타입이 `started`인 이슈를 가져옵니다.

```graphql
query PullInProgress($viewerId: ID!) {
  issues(filter: {
    assignee: { id: { eq: $viewerId } }
    state:    { type: { eq: "started" } }
  }) {
    nodes { id identifier title description url priority
            state { id name type }
            team  { id } }
  }
}
```

가져온 이슈마다 이렇게 처리합니다.

**링크가 없는 이슈**는 로컬 todo로 새로 만듭니다. 제목과 설명을 Linear에서 채우고, 상태는 `in_progress`, 우선순위는 Linear 값을 그대로 씁니다(규약이 같습니다). 그리고 링크를 겁니다.

**이미 링크된 이슈**는 건드리지 않습니다. 제목을 Linear에서 고쳤어도 로컬은 그대로입니다. 이게 "가져오기는 sync가 아니다"의 실제 의미입니다.

**소프트 삭제된 todo에 링크가 남아있는 이슈**도 건드리지 않습니다. 지운 것이 되살아나지 않습니다.

가져오기 결과는 토스트로 알립니다. "새로 3건, 건너뜀 7건".

가져오기와 별개로, 같은 실행에서 역방향 확인을 한 번 합니다. 로컬에 `done`이 아닌 todo 중 Linear 링크가 있는 것들을 모아 상태를 조회하고, Linear에서 `completed`나 `canceled`인 것이 있으면 로컬을 `done`으로 내립니다. 이때는 아웃박스에 아무것도 넣지 않습니다. 이미 Linear가 done이니까요.

이것이 유일한 역방향입니다.

### 밀어넣기

로컬에서 `done`으로 바뀌었고 그 todo에 Linear 링크가 있으면, 상태 변경과 같은 트랜잭션에서 아웃박스에 `linear_complete`를 넣습니다.

워커가 5초마다 `next_attempt_at`이 지난 항목을 꺼내 처리합니다.

처리 순서입니다. 그 todo의 `team_id`로 `settings.linear.done_state.<team_id>`를 찾습니다. 없으면 팀의 워크플로우 상태 중 타입이 `completed`인 것들을 조회합니다. 하나뿐이면 그걸 쓰고 저장합니다. 여럿이면(Done, Merged 같은 경우) 사용자에게 물어보고 답을 저장합니다. 그동안 그 아웃박스 항목은 대기 상태로 남습니다.

상태 ID가 정해지면 `issueUpdate(id:, input:{stateId:})`를 부릅니다. 성공하면 `completed_at`을 찍고 `linear_links.last_pushed_status`를 갱신합니다.

실패하면 `attempts`를 올리고 `next_attempt_at`을 지수 백오프로 밉니다. 5초, 15초, 1분, 5분, 30분. 그 뒤로는 30분 간격을 유지합니다. `last_error`에 사유를 남깁니다.

`attempts`가 3을 넘으면 UI 상단에 "Linear 동기화 실패 N건"을 띄우고, 팔레트에서 `Retry failed Linear syncs`로 즉시 재시도할 수 있게 합니다.

done을 취소해도 Linear로 아무것도 보내지 않습니다. 이미 밀어넣은 뒤라면 Linear는 done인 채로 남습니다. 의도한 동작입니다.

우선순위와 제목 변경은 Linear로 밀지 않습니다. 로컬이 원본이라는 전제와 별개로, 그건 협업 상대에게 소음이 됩니다.

## 8. 바깥 인터페이스

REST와 MCP가 같은 포트(2470)에서 서빙됩니다. axum 라우터 하나에 REST 핸들러들과 rmcp의 `StreamableHttpService`를 함께 마운트합니다. `StreamableHttpService`가 tower `Service`이므로 그대로 붙습니다.

별도 stdio 브릿지 바이너리를 만들지 않습니다.

### REST

모든 경로에 `Authorization: Bearer <token>`이 필요합니다. `/api/v1/health`만 예외입니다.

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/api/v1/health` | 살아있는지 확인. 인증 없음 |
| GET | `/api/v1/todos` | 목록. `status`, `priority`, `due_before`, `q`, `limit`, `offset` |
| POST | `/api/v1/todos` | 생성 |
| GET | `/api/v1/todos/:id` | 단건 |
| PATCH | `/api/v1/todos/:id` | 수정. 보낸 필드만 바뀜 |
| DELETE | `/api/v1/todos/:id` | 소프트 삭제 |
| POST | `/api/v1/todos/:id/restore` | 복구 |
| POST | `/api/v1/todos/:id/link/linear` | `{"issue_ref": "PI-1234"}` |
| POST | `/api/v1/linear/pull` | 가져오기 실행. 결과 요약 반환 |

`q`가 있으면 FTS5로 검색합니다. `due_date`는 자연어를 받습니다.

`priority`는 요청과 응답 모두 문자열입니다. `urgent`, `high`, `medium`, `low`, `none`. 숫자 규약(0=None)은 데이터베이스 안에만 있고 바깥으로 새지 않습니다. MCP도 같습니다.

오류는 `{"error": {"code": "...", "message": "..."}}` 형태로 돌려줍니다. 코드는 `not_found`, `invalid_input`, `linear_not_configured`, `issue_already_linked`, `unauthorized`.

### MCP

도구 아홉 개입니다.

| 도구 | 인자 |
|---|---|
| `todo_list` | status?, priority?, due_before?, query?, limit? |
| `todo_get` | id |
| `todo_create` | title, description?, priority?, due_date? |
| `todo_update` | id, title?, description?, priority?, due_date? |
| `todo_set_status` | id, status |
| `todo_delete` | id |
| `todo_restore` | id |
| `todo_link_linear` | id, issue_ref |
| `linear_pull_in_progress` | 없음 |

삭제를 노출했으니 복구도 노출합니다. 모델이 지운 것을 모델이 되돌릴 수 있어야 합니다.

우선순위는 문자열로 받습니다. `urgent`, `high`, `medium`, `low`, `none`. 모델이 숫자 규약을 외울 필요가 없습니다.

등록은 한 줄입니다.

```
claude mcp add --transport http todo http://127.0.0.1:2470/mcp \
  --header "Authorization: Bearer $(cat ~/.config/todo/token)"
```

앱이 꺼져 있으면 도구 호출이 연결 오류로 실패합니다. 의도한 동작입니다.

## 9. 나중에 붙을 탭

캘린더와 메일은 완전히 별도 화면입니다. 데이터가 섞이지 않습니다. 그래서 지금 준비할 것이 둘뿐입니다.

하나, 탭 셸을 만듭니다. 지금은 Todo 탭 하나만 있습니다.

둘, 단축키를 전역 스코프와 뷰 스코프로 나눕니다.

캘린더 탭이 붙을 때는 `calendar-core` 크레이트와 자기 테이블이 따로 생깁니다. REST는 `/api/v1/calendar/*`로, MCP 도구는 `calendar_` 접두사로 붙습니다. `todos` 테이블은 손대지 않습니다. 마이그레이션이 필요 없습니다.

`todos`에 `source` 컬럼 같은 것을 미리 만들지 않습니다. 지금 쓰지 않는 컬럼이고, 나중에 필요한 모양이 지금 상상한 모양과 다를 것이기 때문입니다.

## 10. 오류 처리

| 상황 | 처리 |
|---|---|
| 2470 포트 점유 | 앱은 뜬다. 상단 배너로 알린다. UI는 IPC를 쓰므로 무관 |
| Linear API 키 없음 | Linear 명령이 팔레트에 안 뜬다. `l` 누르면 안내 |
| Linear API 호출 실패 (밀어넣기) | 아웃박스에 남고 지수 백오프 재시도. 3회 초과 시 배너 |
| Linear API 호출 실패 (가져오기) | 수동 실행이므로 토스트로 실패만 알린다. 재시도 안 함 |
| 팀의 completed 상태가 여럿 | 사용자에게 한 번 묻고 `settings`에 저장. 그때까지 아웃박스 항목은 대기 |
| 이미 링크된 이슈에 다시 링크 | 어느 todo에 걸려 있는지 알려준다 |
| 자연어 날짜 파싱 실패 | 저장하지 않고 오류. 추측하지 않는다 |
| SQLite 잠김 | `busy_timeout` 5초. 그래도 실패하면 오류를 올린다 |

조용히 삼키는 실패를 만들지 않습니다. 특히 Linear 밀어넣기가 그렇습니다. 로컬은 done인데 Linear는 아닌 상태를 사용자가 모르면 안 됩니다.

## 11. 테스트

**todo-core**가 테스트의 중심입니다. 인메모리 SQLite로 유스케이스를 직접 검증합니다.

- 상태 전이와 `completed_at` 처리
- 우선순위 정렬에서 None이 맨 뒤로 가는지
- 소프트 삭제된 항목이 목록·검색에 안 나오는지
- 링크된 todo를 done으로 바꿀 때 아웃박스에 정확히 하나만 쌓이는지 (두 번 눌러도 하나)
- 링크 없는 todo를 done으로 바꿀 때 아웃박스가 비는지
- 자연어 날짜 파싱 (경계: 오늘이 금요일일 때 `fri`, 연말의 `4/20`)
- FTS5 검색이 제목과 설명을 모두 잡는지

**todo-linear**는 `wiremock`으로 GraphQL 응답을 흉내 냅니다.

- 가져오기가 이미 링크된 이슈를 건너뛰는지
- 가져오기가 소프트 삭제된 todo의 이슈를 되살리지 않는지
- Linear에서 completed된 이슈가 로컬을 done으로 내리는지
- 밀어넣기 실패 시 백오프가 늘어나는지
- completed 상태가 여럿일 때 대기 상태로 남는지

**todo-server**는 통합 테스트입니다.

- 토큰이 없으면 401
- `/health`는 토큰 없이 200
- Origin이 다르면 거부
- MCP `tools/list`가 아홉 개를 돌려주는지
- REST가 우선순위를 문자열로 주고받는지. 숫자가 밖으로 새지 않는지

**프론트엔드**는 키 처리기가 순수 함수이므로 그것만 단위 테스트합니다.

- `p` 다음 `u`가 Urgent를 만드는지
- `p` 다음 500ms 뒤에 온 `u`가 아무 일도 안 하는지
- 입력 필드에 포커스가 있을 때 `d`가 죽는지
- 되돌리기 스택이 역동작을 올바로 쌓는지

## 12. 개발 환경

실제로 이 앱을 쓰는 기기는 macOS입니다. 개발은 GUI 없는 리눅스 서버에서 SSH로 합니다.

이 비대칭이 마일스톤 순서를 정합니다. 리눅스에서 `cargo test`는 완전히 됩니다. 브라우저에서 Vite dev server를 보는 것도 포트 포워딩으로 됩니다. 안 되는 것은 딱 하나, Tauri 창을 띄워 눈으로 보는 것입니다.

그래서 Tauri 셸을 맨 뒤로 미룹니다. 그 앞의 모든 단계는 리눅스에서 검증됩니다. Tauri 셸은 얇으므로 Mac에서 마무리하는 비용이 작습니다.

리눅스 개발 서버에 `libwebkit2gtk`를 깔지 않습니다. 필요가 없습니다.

## 13. 마일스톤

**M1 — todo-core.** 순수 Rust. 스키마와 마이그레이션, 도메인 유스케이스, 자연어 날짜 파서, FTS5 검색, 소프트 삭제와 복구, 아웃박스 적재, 도메인 이벤트. `cargo test`로 전부 검증됩니다.

**M2 — todo-server.** axum, 토큰 인증, REST, rmcp MCP. 통합 테스트로 검증하고, `curl`과 MCP 클라이언트로 직접 확인합니다.

**M3 — UI와 단축키.** Solid, `TodoClient` 인터페이스와 `HttpClient` 구현, 리스트, 커맨드 팔레트, 검색, 되돌리기. 브라우저에서 개발하고 확인합니다. 키 처리기는 순수 함수라 `vitest`로 검증합니다.

**M4 — Tauri 셸.** Mac에서 진행합니다. 창, IPC command, `TauriClient` 구현, 도메인 이벤트를 웹뷰로 emit. 서버를 백그라운드 태스크로 띄우기. 여기까지 끝나면 매일 쓸 수 있습니다.

**M5 — Linear.** `todo-linear`, 키체인, GraphQL 클라이언트, 링크, 아웃박스 워커, 가져오기, 밀어넣기.

M3이 끝나기 전에 M5를 손대지 않습니다. 이 앱의 값어치는 단축키 체감에 거의 전부 걸려 있습니다. 그건 M3에서만 만들어집니다. M3을 며칠 써보고 단축키 배치가 손에 안 맞으면 그때 고치는 게, M5를 먼저 만들어놓고 고치는 것보다 훨씬 쌉니다.

M2가 M3보다 앞에 오는 것이 원래 계획과 다른 점입니다. UI를 브라우저에서 개발하려면 REST가 먼저 있어야 하기 때문입니다.
