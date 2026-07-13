# Gmail 이메일 탭 설계

작성일: 2026-07-11

## 1. 목표

todoapp에 이메일 탭을 추가한다. Google Gmail만 지원하고, 여러 Google 계정을
붙일 수 있으며, 기존 Todo 탭과 같은 키보드 중심(Superhuman 스타일) 조작감을
유지한다. 로딩을 progressive하게 처리해 "빠른 이메일 클라이언트를 쓰는 느낌"을
준다. v1은 받은 메일 보기에 집중하며, inbox / archive / 전체보관함(all mail) 세
폴더만 구분한다.

### 요구사항 (사용자 확정)

1. Google만 지원한다.
2. Google 계정을 여러 개 추가할 수 있다.
3. Superhuman 같은 키보드 단축키 컨셉을 그대로 적용한다.
4. progressive 로딩으로 빠른 체감을 준다.
5. 받은 메일 보기 위주로 먼저 만든다. inbox / archive / all-mail만 구분한다.

### v1 쓰기 동작 범위 (확정)

읽기·목록·본문 보기에 더해 다음 쓰기 동작만 포함한다.

- `e`로 보관 (inbox → archive, `INBOX` 라벨 제거)
- 읽음 / 안읽음 토글 (`UNREAD` 라벨 토글)
- 메일 본문을 열면 읽음 처리

답장·작성·영구 삭제·첨부 다운로드·고급 검색은 v1 범위 밖이며 다음 단계로 미룬다.

### 계정 뷰 (확정)

통합 받은메일함으로 보여준다. 모든 계정 메일을 날짜순 한 목록으로 섞되, 어느
계정 메일인지 확실히 인지되도록 계정별 색 스트라이프와 계정 라벨을 붙인다.
컬럼(lane) 분할은 `j/k` 선형 이동을 깨므로 채택하지 않고, 단일 목록 + 계정 색으로
구분한다. 특정 계정만 보는 필터를 제공한다.

## 2. 접근 결정

**A안 — 로컬 캐시 우선 + Gmail REST + History 증분 동기화 (채택)**

메시지 메타데이터와 본문을 로컬 SQLite에 캐시한다. 탭을 열면 캐시를 즉시 렌더하고
(네트워크 대기 0), 백그라운드에서 Gmail `users.history.list`로 변경분만 당겨
목록에 흘려보낸다. 본문은 커서 주변을 미리 당긴다. 재진입이 가장 빠르고, 오프라인
읽기가 되며, 요구사항 4(빠른 느낌)에 가장 잘 맞는다. 기존 `todo-linear`의
키체인·아웃박스·도메인 이벤트 패턴을 그대로 재사용한다.

- B안(라이브 페치)은 열 때마다 네트워크를 타 "빠른 느낌"이 안 나 탈락.
- C안(IMAP)은 Gmail의 inbox/archive/all-mail 라벨 의미와 어긋나고 증분 동기화가
  History API보다 불리해 탈락.

**목록 단위**: v1은 메시지 단위로 목록을 그린다. 스키마에 `thread_id`를 보존해
나중에 스레드 그룹핑을 얹을 수 있게 한다.

**플랫폼**: 메일 기능은 OAuth loopback과 OS 키체인을 쓰므로 본질적으로 데스크톱
기능이다. v1 메일은 Tauri(IPC) 경로만 구현한다. REST(`todo-server`) 노출은 다음
단계로 미룬다. `HttpClient`(개발용 REST) 경로에서 메일 탭은 비활성 상태로 둔다.

## 3. 아키텍처

### 크레이트

새 크레이트 `crates/todo-gmail`을 `todo-linear`와 나란히 둔다.

- `GmailService` (`todo-linear`의 `LinearService`와 동형): `TodoCore`(SQLite 풀
  공유), `reqwest::Client`, OAuth 엔드포인트, `Arc<dyn KeyStore>`, 계정별 동기화
  상태를 들고 있다.
- 재사용: `keyring` 기반 키스토어(계정별 refresh 토큰), `settings` 테이블(client
  id/secret, 계정별 history_id), 아웃박스 패턴, 도메인 이벤트 브로드캐스트 패턴.
- `KeyStore` 트레이트는 현재 `todo-linear`에 있다. `todo-gmail`도 같은 트레이트가
  필요하므로, `KeyStore` / `KeyStoreError` / `SystemKeyStore`를 `todo-core`(또는
  작은 공용 위치)로 옮겨 두 크레이트가 공유한다. 키체인 항목 이름만 다르게 쓴다.

### Tauri 커맨드 (`src-tauri/src/lib.rs`)

`ShellState`에 `gmail: GmailService`를 추가하고 다음 커맨드를 노출한다.

- `gmail_accounts() -> Vec<GmailAccount>` — 등록된 계정과 상태(색, 동기화/토큰 상태).
- `gmail_add_account_start() -> AuthChallenge` — loopback 포트를 열고 동의 URL을 반환.
- `gmail_add_account_finish(...)` — 콜백으로 받은 code를 토큰으로 교환, refresh 토큰
  키체인 저장, 계정 upsert, 초기 동기화 시작. (구현에 따라 start가 콜백까지 대기해
  하나로 합쳐질 수 있다.)
- `gmail_remove_account(account_id)` — 계정·캐시·키체인 항목 제거.
- `gmail_list(filter) -> Vec<MailListItem>` — 로컬 캐시에서 폴더/계정/검색 필터로 조회.
- `gmail_get_body(account_id, gmail_id) -> MailBody` — 캐시 본문 반환, 없으면 페치.
- `gmail_archive(account_id, gmail_id)` — 낙관적 보관 + 아웃박스 적재.
- `gmail_set_read(account_id, gmail_id, read: bool)` — 낙관적 읽음/안읽음 + 아웃박스.
- `gmail_sync()` — 수동 증분 동기화 트리거.
- `gmail_set_client_credentials(client_id, client_secret)` — 설정 저장.

메일 변경(동기화 도착·낙관적 갱신)은 별도 브로드캐스트 채널로 흘려 Tauri 이벤트
`mail:changed`로 프론트에 알린다(todo `todo:changed`와 동형). `GmailService`가
`spawn_worker()`로 (a) 주기 증분 동기화(예: 60초)와 (b) 아웃박스 처리(예: 5초)를
돌린다.

### 프론트엔드

- `src/mail/domain.ts` — `GmailAccount`, `MailListItem`, `MailBody`, `MailFolder`,
  `MailFilter` 타입.
- `src/mail/client.ts` — `GmailClient` 인터페이스 + `TauriGmailClient` 구현
  (`decodeX` 방어적 디코더는 기존 `client.ts` 스타일을 따른다).
- `src/App.tsx` — 탭바에 `Mail` 추가, 탭 상태에 따라 Todo 뷰 / Mail 뷰 스위치.
  메일 뷰는 규모가 있으니 `src/mail/MailView.tsx` 컴포넌트로 분리한다.
- `src/keyboard.ts` — `ShortcutScope`에 `"mail"` 추가, 메일 액션 분기 추가.

## 4. 데이터 모델

`todo-core/migrations`에 새 마이그레이션 파일을 추가한다(예:
`20260711000000_gmail.sql`). 기존 `todos` 계열은 건드리지 않는다.

```sql
CREATE TABLE gmail_accounts (
    id            TEXT PRIMARY KEY,        -- 내부 UUID
    email         TEXT NOT NULL UNIQUE,
    color         TEXT NOT NULL,           -- 계정 구분용 accent (배정 규칙은 §7)
    history_id    TEXT,                    -- 마지막으로 반영한 Gmail historyId
    sync_state    TEXT NOT NULL DEFAULT 'idle',  -- idle | syncing | needs_auth | error
    last_error    TEXT,
    last_synced_at TEXT,
    added_at      TEXT NOT NULL
);

CREATE TABLE gmail_messages (
    account_id      TEXT NOT NULL REFERENCES gmail_accounts(id) ON DELETE CASCADE,
    gmail_id        TEXT NOT NULL,          -- Gmail 메시지 id
    thread_id       TEXT NOT NULL,          -- 스레드 그룹핑용(미래 대비)
    from_name       TEXT NOT NULL DEFAULT '',
    from_email      TEXT NOT NULL DEFAULT '',
    subject         TEXT NOT NULL DEFAULT '',
    snippet         TEXT NOT NULL DEFAULT '',
    internal_date   INTEGER NOT NULL,       -- Gmail internalDate(ms), 정렬 키
    in_inbox        INTEGER NOT NULL DEFAULT 0,  -- INBOX 라벨 유무
    is_unread       INTEGER NOT NULL DEFAULT 0,  -- UNREAD 라벨 유무
    body_text       TEXT,                   -- 지연 로드, NULL = 미페치
    body_html       TEXT,
    body_fetched_at TEXT,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (account_id, gmail_id)
);

CREATE INDEX idx_gmail_msgs_list
    ON gmail_messages(internal_date DESC);
CREATE INDEX idx_gmail_msgs_inbox
    ON gmail_messages(in_inbox, internal_date DESC);

CREATE TABLE gmail_outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      TEXT NOT NULL REFERENCES gmail_accounts(id) ON DELETE CASCADE,
    gmail_id        TEXT NOT NULL,
    kind            TEXT NOT NULL,          -- archive | mark_read | mark_unread
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    last_error      TEXT,
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE UNIQUE INDEX idx_gmail_outbox_pending
    ON gmail_outbox(account_id, gmail_id, kind) WHERE completed_at IS NULL;
CREATE INDEX idx_gmail_outbox_ready
    ON gmail_outbox(next_attempt_at) WHERE completed_at IS NULL;
```

폴더 판정:

- **inbox**: `in_inbox = 1`
- **archive**: `in_inbox = 0` (동기화 대상에서 SPAM/TRASH를 제외하므로, 남은 것 중
  inbox가 아닌 것이 곧 보관함)
- **all**: 계정에 대해 캐시된 전부

`gmail.modify` 스코프로 라벨을 바꾸므로 archive = `INBOX` 라벨 제거, 읽음 =
`UNREAD` 제거, 안읽음 = `UNREAD` 추가로 대응한다.

키체인·설정 키:

- refresh 토큰: 키체인 `service="todo"`, `user="gmail-refresh:{email}"`
- OAuth client: `settings["gmail.client_id"]`, `settings["gmail.client_secret"]`
  (데스크톱 설치형 앱의 secret은 진짜 비밀이 아니나, 사용자 입력값을 그대로 보관)

## 5. 동기화와 progressive 로딩

### 초기 계정 추가 시

1. OAuth 완료 후 refresh 토큰 저장, 계정 row upsert.
2. 초기 동기화: `users.messages.list`로 최근 메시지 id를 당긴다. 초기 부하와 저장을
   묶기 위해 최근 창(예: `newer_than:90d`)과 상한(계정당 수백 통)을 둔다.
   SPAM/TRASH는 제외한다. 먼저 INBOX부터, 그다음 나머지 All Mail 순으로 채운다.
3. id 배치를 `messages.get(format=metadata)`(헤더: From/Subject/Date/라벨)로 당겨
   메타 row를 upsert하고, 도착하는 대로 `mail:changed`를 쏴 목록에 흘려보낸다.
4. 응답의 `historyId`(가장 최근 메시지 기준)를 계정에 저장한다.

### 이후 증분 동기화

- 워커가 주기적으로 계정별 `users.history.list(startHistoryId)`를 호출한다.
- 반환된 messagesAdded / labelsAdded / labelsRemoved / messagesDeleted를 로컬에
  반영한다(신규 메타 페치, `in_inbox`/`is_unread` 갱신, 삭제 정리). 반영 후
  `historyId`를 전진시키고 `mail:changed`를 쏜다.
- `historyId`가 만료(410)되면 초기 동기화 로직으로 폴백한다.

### 본문 지연 로드 + 프리페치

- 목록은 메타만으로 그린다(본문 없이도 즉시 렌더).
- 커서가 놓인 메일과 위아래 몇 통(예: ±3)의 본문을 `messages.get(format=full)`로
  미리 당겨 `body_*`를 채운다. 열면 이미 로컬에 있어 즉시 표시된다.
- 캐시에 본문이 없고 프리페치도 안 된 상태로 열면 그때 페치하고, 그동안 스니펫을
  보여준다.

### 쓰기(보관 / 읽음)의 낙관적 처리

1. 로컬 캐시를 즉시 갱신(`in_inbox`/`is_unread`)하고 `mail:changed`로 UI 반영.
2. `gmail_outbox`에 해당 동작을 적재한다.
3. 아웃박스 워커가 `users.messages.modify`로 Gmail에 반영한다. 성공하면 완료 표시,
   재시도 가능한 실패는 백오프 후 재시도(기존 `todo-linear` 백오프 로직 재사용),
   비재시도 실패는 로컬 롤백 + 사용자 알림.

## 6. 인증과 계정

### OAuth loopback 흐름 (설치형 데스크톱 앱)

1. 앱이 임시 `127.0.0.1:<ephemeral>` HTTP 리스너를 연다(최소 리스너; `todo-server`가
   이미 tokio/axum을 쓰므로 작은 핸들러 하나로 충분).
2. 시스템 브라우저로 Google 동의 URL을 연다
   (`https://accounts.google.com/o/oauth2/v2/auth`, `scope=.../auth/gmail.modify`,
   `access_type=offline`, `prompt=consent`, PKCE 사용).
3. 사용자가 계정을 고르고 동의하면 Google이 loopback으로 리다이렉트하며 `code`를 준다.
4. `code`를 `https://oauth2.googleapis.com/token`에서 refresh/access 토큰으로 교환한다.
5. refresh 토큰을 계정별 키체인에 저장한다. access 토큰은 메모리 캐시(만료 시 refresh로
   재발급).

### 다계정

- 계정 추가를 반복하면 계정마다 refresh 토큰이 키체인에 쌓인다.
- 계정 각각에 대해 독립적으로 동기화·프리페치·아웃박스를 돌린다.

### 토큰 만료 (개인 Gmail 포함 제약)

개인 Gmail을 섞으므로 OAuth 앱을 워크스페이스 "내부(Internal)"로 둘 수 없고
"외부(External)/테스트" 모드가 된다. 이 모드에서는 refresh 토큰이 약 7일마다
만료된다(워크스페이스 계정 포함). 따라서:

- refresh 실패(invalid_grant)를 감지하면 계정 `sync_state`를 `needs_auth`로 두고
  UI에 재인증 배지를 띄운다.
- 재인증은 단축키 한 번(또는 배지 클릭)으로 §6.1 흐름을 다시 태워 끝낸다.
- (워크스페이스 계정만 무만료로 쓰려면 OAuth 앱을 Internal/External 2개로 나눠야
  하는데 v1엔 과하므로 채택하지 않는다.)

### OAuth 설정 가이드 (사용자에게 안내)

사용자가 "설정 과정 안내"를 택했으므로, 앱 최초 사용 시(또는 문서로) 아래를 단계별
안내한다.

1. Google Cloud Console에서 프로젝트 생성.
2. "Gmail API" 사용 설정.
3. OAuth 동의 화면 구성: User type = External, 게시 상태 = 테스트, 스코프에
   `.../auth/gmail.modify` 추가, 사용할 Google 계정들을 "테스트 사용자"로 등록.
4. 사용자 인증 정보 → OAuth 클라이언트 ID → 애플리케이션 유형 = **데스크톱 앱** 생성.
5. 발급된 client ID / client secret을 앱 설정 화면에 입력.
6. 이후 "계정 추가"로 각 Google 계정을 로그인.

## 7. UI와 키보드

### 레이아웃

- 상단 탭바: `Todo` | `Mail` (기존 tabbar 확장). 탭 전환은 클릭 + 단축키.
- 메일 뷰: 폴더 필터 행(inbox / archive / all) + 계정 필터 + 통합 목록 + 우측
  상세(본문) 패널(기존 detail-panel 패턴 재사용).
- 목록 각 행: 왼쪽 계정 색 스트라이프, 발신자, 제목, 스니펫, 날짜, 안읽음 표시,
  계정 라벨(짧은 이메일 또는 별칭). 계정 색은 등록 순서로 팔레트에서 순환 배정하고
  계정 설정에서 바꿀 수 있게 한다.
- 상단에 동기화 진행 표시(스피너 + "N통 불러오는 중")로 progressive 체감을 준다.

### 키보드 (`mail` 스코프)

Todo 스코프와 별개 분기다(`keyboard.ts`는 `topScope`로 갈린다).

| 키 | 동작 |
| --- | --- |
| `j` / `k` | 아래 / 위 이동 |
| `Enter` | 본문 열기 (열면 읽음 처리) |
| `Esc` | 본문 닫기 / 선택 해제 |
| `e` | 보관 (inbox → archive) |
| `u` | 읽음 / 안읽음 토글 |
| `1` / `2` / `3` | inbox / archive / all 필터 |
| `/` | 로컬 검색 |
| `⌘K` / `Ctrl+K` | 커맨드 팔레트(메일 명령 포함) |
| `?` | 단축키 도움말 |

- Todo의 `u`(undo)와 `mail`의 `u`(읽음 토글)는 스코프가 달라 충돌하지 않는다.
- 탭 전환 단축키를 하나 둔다(예: `g` 후 `t`/`m`, 또는 전용 키). 구체 매핑은 계획
  단계에서 기존 chord 패턴과 맞춰 확정한다.
- 커맨드 팔레트에 "Add Google account", "Sync mail", "Re-authenticate <account>"
  명령을 추가한다.

## 8. 오류 처리

- 키체인을 못 열거나 client 자격증명이 없으면 메일 탭은 "설정 필요" 안내를 띄우고
  Todo 기능에는 영향을 주지 않는다(`todo-linear`가 키체인 실패를 미설정과 같게 다루는
  방침을 따른다).
- Gmail API 오류는 재시도 가능/불가로 나눈다(429/5xx·네트워크 = 재시도, 4xx 대부분 =
  비재시도). 아웃박스 백오프는 `todo-linear`와 동일 로직을 재사용한다.
- refresh 실패는 `needs_auth`로 전환하고 재인증을 유도한다(§6.3).
- 낙관적 쓰기가 최종 실패하면 로컬 상태를 되돌리고 사용자에게 알린다.

## 9. 테스트 전략

- `todo-gmail` 단위 테스트(모의 HTTP 엔드포인트, `todo-linear` 테스트 방식 차용):
  라벨 → 폴더 매핑, history 델타 적용, 초기 동기화, 아웃박스 modify 처리와 백오프,
  토큰 교환/리프레시 로직.
- `keyboard.ts`: `mail` 스코프 케이스를 `keyboard.test.ts` / `keyboard.table.test.ts`에
  추가(이동, Enter, e, u, 필터, Esc).
- 프론트 클라이언트: `MailListItem`/`MailBody` 방어적 디코더 테스트.
- OAuth loopback 전체 흐름은 E2E가 어렵다. 토큰 교환/리프레시는 모의 토큰 엔드포인트로
  단위 검증하고, 브라우저 동의 단계는 수동 확인으로 남긴다.

## 10. v1 범위 정리

**포함**

- 구글 다계정 로그인(loopback OAuth, 계정별 키체인)
- inbox / archive / all 폴더 보기
- 통합 목록(계정 색·라벨 구분) + 계정 필터
- 메시지 본문 보기(지연 로드 + 프리페치)
- `e` 보관, 읽음/안읽음 토글, 열람 시 읽음 처리
- progressive 로딩(로컬 캐시 즉시 렌더 + 백그라운드 증분 동기화)
- 낙관적 쓰기 + 아웃박스 반영
- 재인증 흐름(7일 만료 대응)

**제외 (다음 단계)**

- 답장·전달·새 메일 작성
- 영구 삭제, 스팸/휴지통 관리
- 첨부 파일 다운로드·미리보기
- 스레드(대화) 그룹핑 — 스키마만 대비(`thread_id`)
- 실시간 push(Gmail watch/Pub/Sub) — v1은 주기·수동 동기화
- 서버 사이드(REST/MCP) 메일 노출 — v1은 Tauri 전용
- 고급 검색(원격 Gmail 검색). v1 검색은 로컬 캐시 대상

## 11. 미해결 / 계획 단계에서 확정할 것

- 탭 전환·재인증 단축키의 구체 키 매핑(기존 chord 패턴과 정합).
- `KeyStore`를 `todo-core`로 옮길지, 별도 작은 공용 크레이트로 뺄지.
- 초기 동기화 창(`newer_than`)과 계정당 상한의 구체 수치.
- 계정 색 팔레트 값과 재정의 UI 범위.
