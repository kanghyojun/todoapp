# 안읽은 메일 뱃지 — 설계

작성: 2026-07-14
브랜치: `feature/m4-tauri-shell`

## 목표

안읽은 새 메일이 있는지 한눈에 알 수 있게 한다. OS 푸시 알림은 만들지 않는다.
받은편지함 안읽음 개수를 두 곳에 보여준다.

- 상단 **Mail 탭 라벨 옆 뱃지**
- **macOS Dock 카운트 뱃지**

## 무엇을 셀 것인가

받은편지함 안읽음만 센다. 즉 `in_inbox = 1 AND is_unread = 1` 인 메시지다.
보관(archive)한 안읽음은 제외한다. 전 계정을 합산한다. 두 뱃지가 같은 수를 쓴다.

## 접근 방식

백엔드가 개수의 단일 출처가 된다(A안).

- 개수 계산 쿼리는 백엔드에 하나만 둔다.
- Dock 뱃지는 백엔드가 직접 세팅한다. 웹뷰 상태와 무관하게 항상 정확하다.
- Mail 탭 뱃지는 프론트가 같은 개수를 얇게 읽어 그린다.
- `mail:changed` 이벤트 계약은 그대로 둔다. 개수를 이벤트에 싣지 않는다("바뀜" 신호만).

대안으로 이벤트에 개수를 싣는 B안, 프론트가 Dock까지 모는 C안이 있으나,
B는 범용 이벤트에 개수를 결합해 계약이 지저분해지고, C는 웹뷰가 느리면 Dock이
뒤처지고 목록 조회가 낭비다. A가 관심사를 깔끔히 나눈다.

## 구성 요소

### 백엔드 (Rust)

1. `todo-gmail/src/store.rs`
   - `count_inbox_unread(pool) -> Result<i64, Error>`
   - `SELECT COUNT(*) FROM gmail_messages WHERE in_inbox = 1 AND is_unread = 1`

2. `todo-gmail/src/service.rs`
   - `unread_count(&self) -> Result<u64, Error>` — 위 스토어 함수를 래핑한다.

3. `src-tauri/src/lib.rs`
   - 커맨드 `gmail_unread_count() -> Result<u64, CommandError>` 추가, `invoke_handler` 등록.
   - `refresh_dock_badge(app, gmail)` 헬퍼(macOS `#[cfg]`):
     개수를 구해 `get_webview_window("main").set_badge_count(n > 0 ? Some(n) : None)`.
     실패는 삼킨다(best-effort). 비 macOS 에서는 no-op.
   - `forward_mail_events` 루프의 매 이벤트마다 Dock 뱃지 재계산·세팅.
   - 앱 시작 시(초기 동기화 뒤) 1회 세팅.

### 프론트엔드 (TS / Solid)

4. `src/mail/client.ts`
   - `GmailClient.unreadCount(): Promise<number>` 인터페이스 추가.
   - Tauri 구현: `gmail_unread_count` 호출, number 가드.

5. `src/App.tsx`
   - 시그널 `mailUnread`(number)와 `refreshMailUnread()`.
     gmailClient 있으면 개수 조회, 오류는 삼킨다.
   - `onMount` 에서 1회 + `props.gmailClient.subscribe(...)` 로 `mail:changed`
     때마다 갱신한다. (지금 App 은 todo 클라이언트만 구독하니 gmail 구독을 더한다.)
   - Mail 탭 버튼에 `<Show when={mailUnread() > 0}>` 로 뱃지 렌더. 99 초과는 `99+`.

6. `src/styles.css`
   - `.tab-badge` 작은 알약 스타일.

## 데이터 흐름

동기화·읽음토글·보관 → 서비스 `emit()` → `MailEvent::Changed`
→ `forward_mail_events` 루프. 루프가 두 가지를 한다.

- Dock 뱃지 재계산·세팅
- `mail:changed` 를 웹뷰로 전달

웹뷰의 App 이 `mail:changed` 를 받아 개수를 다시 읽어 탭 뱃지를 갱신한다.

## 이상 입력·경계

- gmailClient 없음 / 계정 0 / 안읽음 0 → 탭 뱃지 숨김, Dock 은
  `set_badge_count(None)` 으로 지운다.
- 개수 쿼리 실패 → 직전 값 유지, 앱을 막지 않는다. Dock 세팅 실패도 삼킨다.
- MailView 는 읽음 처리를 로컬 낙관 반영한다. 권위 개수는 `emit()` 뒤 백엔드에서
  오므로 한 틱 안에 탭·Dock 이 함께 맞춰진다.
- Windows/Linux 는 Dock 카운트 미지원(`set_badge_count` 은 macOS/iOS 전용).
  탭 뱃지는 전 플랫폼 동작. 비 macOS 에서 Dock 부분만 no-op.

## 테스트

- Rust: `count_inbox_unread` 스토어 단위 테스트. in_inbox·is_unread 조합을 삽입하고
  개수를 검증한다. `crates/todo-gmail/tests/gmail.rs` 하네스 패턴을 따른다.
- Rust: `unread_count` 서비스 테스트(보관하면 개수가 준다 등).
- TS: `src/mail/client.test.ts` 에 `unreadCount` 호출/디코드 테스트.
- Dock `set_badge_count` 는 Tauri 부작용이라 단위 테스트 대신 `pnpm tauri dev` 로
  육안 검증한다.
