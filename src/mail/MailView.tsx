import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  type Component,
} from "solid-js";
import type { GmailClient } from "./client";
import type { GmailAccount, MailBody, MailFolder, MailListItem } from "./domain";
import { handleMailKey, type MailKeyAction } from "./keyboard-mail";

interface MailViewProps {
  client: GmailClient;
  // App 이 관리하는 ⌘ 눌림 상태. 폴더 칩에 개수 대신 힌트를 보일 때 쓴다.
  metaHeld: boolean;
  onCreateTodo: (item: MailListItem) => Promise<void>;
  // 메일 상세가 열렸는지 App 에 알린다. App 이 할 일 상세를 옆으로 밀어
  // 두 드로어가 겹치지 않고 나란히 서게 한다.
  onDetailOpenChange?: (open: boolean) => void;
}

const FOLDERS: readonly [MailFolder, string, string][] = [
  ["inbox", "inbox", "1"],
  ["archive", "archive", "2"],
  ["all", "all", "3"],
];

function messageFrom(error: unknown): string {
  return error instanceof Error ? error.message : "알 수 없는 오류가 발생했습니다.";
}

function isTextTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.matches("input, textarea") || target.isContentEditable)
  );
}

function accountLabel(email: string): string {
  return email.split("@")[0] ?? email;
}

function formatDate(ms: number): string {
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export const MailView: Component<MailViewProps> = (props) => {
  const [accounts, setAccounts] = createSignal<GmailAccount[]>([]);
  const [messages, setMessages] = createSignal<MailListItem[]>([]);
  const [folder, setFolder] = createSignal<MailFolder>("inbox");
  const [accountFilter, setAccountFilter] = createSignal<string | undefined>();
  const [cursor, setCursor] = createSignal(0);
  const [openId, setOpenId] = createSignal<string | null>(null);
  const [body, setBody] = createSignal<MailBody | null>(null);
  const [search, setSearch] = createSignal("");
  const [searchMode, setSearchMode] = createSignal(false);
  const [syncing, setSyncing] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [showSettings, setShowSettings] = createSignal(false);
  const [clientId, setClientId] = createSignal("");
  const [clientSecret, setClientSecret] = createSignal("");
  // 폴더 칩에 보여줄 개수. 현재 계정 필터 기준, 검색과는 무관하다.
  const [folderCounts, setFolderCounts] = createSignal<Record<MailFolder, number>>({
    inbox: 0,
    archive: 0,
    all: 0,
  });

  const prefetched = new Set<string>();
  let searchInput: HTMLInputElement | undefined;
  let listRequest = 0;

  const current = createMemo(() => messages()[cursor()]);
  const openMessage = createMemo(() =>
    messages().find((message) => message.gmail_id === openId()),
  );
  const needsAuthAccounts = createMemo(() =>
    accounts().filter((account) => account.sync_state === "needs_auth"),
  );

  async function reload(): Promise<void> {
    const request = ++listRequest;
    try {
      const next = await props.client.list({
        folder: folder(),
        account_id: accountFilter(),
        q: search().trim() || undefined,
      });
      if (request !== listRequest) return;
      setMessages(next);
      setError(null);
      if (cursor() >= next.length) {
        setCursor(Math.max(0, next.length - 1));
      }
      prefetchAround();
      void loadFolderCounts();
    } catch (reason) {
      if (request === listRequest) setError(messageFrom(reason));
    }
  }

  // 폴더별 개수. folder="all" 로 현재 계정의 전체를 받아 in_inbox 로 나눈다.
  // 검색과는 독립이다(전체 기준). limit 은 넉넉히 잡아 캡에 걸리지 않게 한다.
  async function loadFolderCounts(): Promise<void> {
    try {
      const all = await props.client.list({
        folder: "all",
        account_id: accountFilter(),
        limit: 1000,
      });
      const inbox = all.filter((message) => message.in_inbox).length;
      setFolderCounts({ inbox, archive: all.length - inbox, all: all.length });
    } catch {
      // 개수는 부가 정보다. 실패해도 목록을 막지 않는다.
    }
  }

  function folderCount(folder: MailFolder): number {
    return folderCounts()[folder];
  }

  async function reloadAccounts(): Promise<void> {
    try {
      setAccounts(await props.client.accounts());
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  function prefetchAround(): void {
    const list = messages();
    const center = cursor();
    for (let index = center - 1; index <= center + 3; index += 1) {
      const item = list[index];
      if (item === undefined || prefetched.has(item.gmail_id)) continue;
      prefetched.add(item.gmail_id);
      void props.client.getBody(item.account_id, item.gmail_id).catch(() => {
        prefetched.delete(item.gmail_id);
      });
    }
  }

  async function openCurrent(): Promise<void> {
    const item = current();
    if (item === undefined) return;
    setOpenId(item.gmail_id);
    setBody(null);
    try {
      setBody(await props.client.getBody(item.account_id, item.gmail_id));
      if (item.is_unread) {
        void markRead(item, true);
      }
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  // 메일 상세 열림/닫힘을 App 으로 흘려보낸다.
  createEffect(() => {
    props.onDetailOpenChange?.(openMessage() !== undefined);
  });

  async function markRead(item: MailListItem, read: boolean): Promise<void> {
    // 낙관적 로컬 반영.
    setMessages((list) =>
      list.map((message) =>
        message.gmail_id === item.gmail_id
          ? { ...message, is_unread: !read }
          : message,
      ),
    );
    try {
      await props.client.setRead(item.account_id, item.gmail_id, read);
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  async function archiveCurrent(): Promise<void> {
    const item = current();
    if (item === undefined) return;
    // inbox 뷰에서는 낙관적으로 목록에서 제거한다.
    if (folder() === "inbox") {
      setMessages((list) =>
        list.filter((message) => message.gmail_id !== item.gmail_id),
      );
    }
    if (openId() === item.gmail_id) setOpenId(null);
    try {
      await props.client.archive(item.account_id, item.gmail_id);
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  function changeFolder(next: MailFolder): void {
    setFolder(next);
    setCursor(0);
    setOpenId(null);
    void reload();
  }

  function beginSearch(): void {
    setSearchMode(true);
    queueMicrotask(() => searchInput?.focus());
  }

  function endSearch(): void {
    setSearchMode(false);
    if (search() !== "") {
      setSearch("");
      void reload();
    }
  }

  function runMailAction(action: MailKeyAction): void {
    switch (action.type) {
      case "Move": {
        const length = messages().length;
        if (length === 0) return;
        const next = Math.min(Math.max(cursor() + action.delta, 0), length - 1);
        setCursor(next);
        prefetchAround();
        break;
      }
      case "Open":
        void openCurrent();
        break;
      case "Close":
        if (openId() !== null) {
          setOpenId(null);
        } else if (searchMode()) {
          endSearch();
        }
        break;
      case "Archive":
        void archiveCurrent();
        break;
      case "ToggleRead": {
        const item = current();
        if (item !== undefined) void markRead(item, item.is_unread);
        break;
      }
      case "CreateTodo": {
        const item = current();
        if (item === undefined) return;
        setMessages((list) =>
          list.map((message) =>
            message.gmail_id === item.gmail_id && message.account_id === item.account_id
              ? { ...message, has_todo: true }
              : message,
          ),
        );
        void props.onCreateTodo(item).finally(() => void reload());
        break;
      }
      case "CycleAccount":
        cycleAccount();
        break;
      case "SetFolder":
        changeFolder(action.folder);
        break;
      case "OpenSearch":
        beginSearch();
        break;
    }
  }

  // 계정 필터를 전체(undefined) → 계정1 → 계정2 → … → 전체 순으로 돌린다.
  function cycleAccount(): void {
    const ids: (string | undefined)[] = [undefined, ...accounts().map((account) => account.id)];
    if (ids.length <= 1) return;
    const index = ids.indexOf(accountFilter());
    const next = ids[(index + 1) % ids.length];
    setAccountFilter(next);
    setCursor(0);
    setOpenId(null);
    void reload();
  }

  function onKeyDown(event: KeyboardEvent): void {
    // Alt 조합과 ⌘K/?·탭 전환은 App 전역 핸들러 몫이다. 폴더 전환(⌘1/2/3)만
    // 여기서 처리하고, 나머지 ⌘/Ctrl 조합은 handleMailKey 가 null 을 내 전역으로 흘린다.
    if (event.altKey) return;
    // 팔레트·도움말 오버레이가 떠 있으면 App 이 처리한다.
    if (document.querySelector(".overlay-backdrop") !== null) return;
    const focus = isTextTarget(event.target) ? "text" : "other";
    const meta = event.metaKey || event.ctrlKey;
    const action = handleMailKey({ focus, detailOpen: openId() !== null, meta }, event.key);
    if (action !== null) {
      event.preventDefault();
      runMailAction(action);
    }
  }

  async function addAccount(): Promise<void> {
    try {
      await props.client.addAccount();
      await reloadAccounts();
      await reload();
    } catch (reason) {
      const code =
        reason instanceof Error && "code" in reason
          ? (reason as { code?: string }).code
          : undefined;
      if (code === "gmail_not_configured") {
        setShowSettings(true);
        setError("먼저 OAuth 클라이언트를 설정하십시오.");
      } else {
        setError(messageFrom(reason));
      }
    }
  }

  async function saveCredentials(): Promise<void> {
    try {
      await props.client.setCredentials(clientId().trim(), clientSecret().trim());
      setShowSettings(false);
      setClientSecret("");
      setError(null);
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  async function triggerSync(): Promise<void> {
    setSyncing(true);
    try {
      await props.client.sync();
    } catch (reason) {
      setError(messageFrom(reason));
    } finally {
      setSyncing(false);
    }
  }

  onMount(() => {
    void reloadAccounts();
    void reload();
    void triggerSync();
    const unsubscribe = props.client.subscribe(() => {
      void reload();
      void reloadAccounts();
    });
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => {
      unsubscribe();
      window.removeEventListener("keydown", onKeyDown);
      // 탭을 벗어나 MailView 가 사라지면 메일 상세도 닫힌 것이다.
      props.onDetailOpenChange?.(false);
    });
  });

  return (
    <main class="mail-view">
      <div class="mail-toolbar">
        <div class="filter-row" aria-label="메일 폴더">
          <For each={FOLDERS}>
            {([value, label, key]) => (
              <button
                type="button"
                class="filter-chip"
                classList={{ active: folder() === value }}
                aria-pressed={folder() === value}
                onClick={() => changeFolder(value)}
              >
                {label}
                <Show when={props.metaHeld} fallback={<span class="chip-count">{folderCount(value)}</span>}>
                  <kbd>⌘{key}</kbd>
                </Show>
              </button>
            )}
          </For>
        </div>

        <div class="mail-account-filter" aria-label="계정 필터">
          <button
            type="button"
            class="filter-chip"
            classList={{ active: accountFilter() === undefined }}
            onClick={() => {
              setAccountFilter(undefined);
              void reload();
            }}
          >
            전체 계정
          </button>
          <For each={accounts()}>
            {(account) => (
              <button
                type="button"
                class="filter-chip mail-account-chip"
                classList={{ active: accountFilter() === account.id }}
                onClick={() => {
                  setAccountFilter(account.id);
                  void reload();
                }}
              >
                <span
                  class="account-dot"
                  aria-hidden="true"
                  style={{ background: account.color }}
                />
                {accountLabel(account.email)}
              </button>
            )}
          </For>
        </div>

        <div class="mail-toolbar-end">
          <Show when={syncing()}>
            <span class="mail-sync" role="status">
              동기화 중…
            </span>
          </Show>
          <button type="button" class="mail-button" onClick={() => void triggerSync()}>
            동기화
          </button>
          <button type="button" class="mail-button" onClick={() => void addAccount()}>
            계정 추가
          </button>
          <button
            type="button"
            class="mail-button"
            onClick={() => setShowSettings((value) => !value)}
          >
            설정
          </button>
        </div>
      </div>

      <Show when={needsAuthAccounts().length > 0}>
        <div class="mail-reauth" role="alert">
          <For each={needsAuthAccounts()}>
            {(account) => (
              <span>
                {account.email} 재인증이 필요합니다.
                <button type="button" class="mail-link" onClick={() => void addAccount()}>
                  재인증
                </button>
              </span>
            )}
          </For>
        </div>
      </Show>

      <Show when={showSettings()}>
        <div class="mail-settings">
          <p>
            Google Cloud에서 만든 <strong>데스크톱 앱</strong> OAuth 클라이언트의 ID와
            secret을 입력하십시오. (Gmail API 사용 설정 + 동의 화면 테스트 사용자 등록이
            선행되어야 합니다.)
          </p>
          <label>
            client_id
            <input
              value={clientId()}
              onInput={(event) => setClientId(event.currentTarget.value)}
              placeholder="xxxx.apps.googleusercontent.com"
            />
          </label>
          <label>
            client_secret
            <input
              value={clientSecret()}
              type="password"
              onInput={(event) => setClientSecret(event.currentTarget.value)}
            />
          </label>
          <button type="button" class="mail-button" onClick={() => void saveCredentials()}>
            저장
          </button>
        </div>
      </Show>

      <Show when={searchMode()}>
        <div class="inline-editor search-editor">
          <label for="mail-search">Search</label>
          <input
            id="mail-search"
            ref={searchInput}
            value={search()}
            placeholder="발신자·제목·본문"
            onInput={(event) => {
              setSearch(event.currentTarget.value);
              void reload();
            }}
          />
          <kbd>Esc</kbd>
        </div>
      </Show>

      <Show when={error()}>
        {(message) => (
          <div class="error-banner" role="alert">
            {message()}
          </div>
        )}
      </Show>

      <section class="mail-list" aria-label="메일 목록">
        <Show
          when={messages().length > 0}
          fallback={
            <div class="empty-state">
              표시할 메일이 없습니다. 계정을 추가하고 동기화하십시오.
            </div>
          }
        >
          <div role="listbox">
            <For each={messages()}>
              {(item, index) => (
                <button
                  type="button"
                  role="option"
                  class="mail-row"
                  classList={{
                    current: cursor() === index(),
                    unread: item.is_unread,
                  }}
                  aria-current={cursor() === index() ? "true" : undefined}
                  style={{ "border-left": `3px solid ${item.account_color}` }}
                  onClick={() => {
                    setCursor(index());
                    void openCurrent();
                  }}
                >
                  <div class="mail-row-line mail-row-top">
                    <span class="mail-subject">{item.subject || "(제목 없음)"}</span>
                    <Show when={item.has_todo}>
                      <span class="mail-todo-tag">✓할일</span>
                    </Show>
                    <time class="mail-date">{formatDate(item.internal_date)}</time>
                  </div>
                  <div class="mail-row-line mail-row-bottom">
                    <span class="mail-from">{item.from_name || item.from_email}</span>
                    <span class="mail-snippet">{item.snippet}</span>
                    <span class="mail-account-tag">{accountLabel(item.account_email)}</span>
                  </div>
                </button>
              )}
            </For>
          </div>
        </Show>
      </section>

      <aside
        class="detail-panel mail-detail"
        classList={{ open: openMessage() !== undefined }}
        aria-hidden={openMessage() === undefined}
      >
        <Show when={openMessage()}>
          {(item) => (
            <>
              <div class="panel-header">
                <span>MAIL</span>
                <kbd>Esc</kbd>
              </div>
              <h1>{item().subject || "(제목 없음)"}</h1>
              <p class="mail-detail-from">
                {item().from_name} &lt;{item().from_email}&gt;
              </p>
              <div class="mail-body">
                <Show
                  when={body()}
                  fallback={<p class="mail-loading">본문을 불러오는 중…</p>}
                >
                  {(loaded) => (
                    <Show
                      when={loaded().body_text}
                      fallback={
                        <Show
                          when={loaded().body_html}
                          fallback={<p>본문이 없습니다.</p>}
                        >
                          {(html) => (
                            <iframe
                              class="mail-body-html"
                              sandbox=""
                              srcdoc={html()}
                              title="메일 본문"
                            />
                          )}
                        </Show>
                      }
                    >
                      {(text) => <pre class="mail-body-text">{text()}</pre>}
                    </Show>
                  )}
                </Show>
              </div>
            </>
          )}
        </Show>
      </aside>
    </main>
  );
};
