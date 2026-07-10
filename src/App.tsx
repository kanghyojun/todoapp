import {
  For,
  Show,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  type Component,
} from "solid-js";
import type { TodoClient } from "./client";
import type { Filter, Priority, Status, Todo } from "./domain";
import {
  handleKey,
  type Action,
  type InputMode,
  type KeyEvent as ShortcutKeyEvent,
  type ShortcutScope,
} from "./keyboard";
import {
  applyPreference,
  cyclePreference,
  readPreference,
  savePreference,
  type ThemePreference,
} from "./theme";
import { UndoStack, type InverseAction } from "./undo";

interface AppProps {
  client: TodoClient;
  externalServerError?: string;
}

type Overlay = "palette" | "help" | null;

type PaletteCommand = "create" | "pull-linear" | "help" | "theme";

type PaletteItem =
  | { kind: "command"; id: PaletteCommand; label: string; hint: string }
  | { kind: "todo"; todo: Todo };

const STATUS_LABELS: Record<Status, string> = {
  todo: "todo",
  in_progress: "in progress",
  done: "done",
};

const PRIORITY_LABELS: Record<Priority, string> = {
  none: "None",
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
};

const COMMANDS: readonly PaletteItem[] = [
  { kind: "command", id: "create", label: "New todo", hint: "c" },
  {
    kind: "command",
    id: "pull-linear",
    label: "Pull Linear issues",
    hint: "M5",
  },
  { kind: "command", id: "theme", label: "Toggle theme", hint: "" },
  { kind: "command", id: "help", label: "Shortcut help", hint: "?" },
];

const THEME_LABELS: Record<ThemePreference, string> = {
  auto: "자동",
  light: "밝게",
  dark: "어둡게",
};

const SHORTCUTS: readonly [string, string][] = [
  ["j / k", "아래 / 위로 이동"],
  ["Enter / Esc", "상세 열기 / 닫기·취소·선택 해제"],
  ["c", "새 todo. Shift+Enter로 연속 생성"],
  ["e", "제목 편집"],
  ["d / i", "done / in progress 토글"],
  ["p u·h·m·l·n", "우선순위 지정"],
  ["t", "마감일 입력"],
  ["x", "선택 토글"],
  ["Backspace", "삭제"],
  ["l / o", "Linear 이슈 연결 / 열기"],
  ["1 / 2 / 3 / 0", "todo / in progress / done / 전체"],
  ["/", "검색"],
  ["u", "되돌리기"],
  ["⌘K / Ctrl+K", "커맨드 팔레트"],
  ["?", "단축키 도움말"],
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

export const App: Component<AppProps> = (props) => {
  const [todos, setTodos] = createSignal<Todo[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [toast, setToast] = createSignal<string | null>(null);
  const [statusFilter, setStatusFilter] = createSignal<Status | undefined>();
  const [searchQuery, setSearchQuery] = createSignal("");
  const [cursorIndex, setCursorIndex] = createSignal(0);
  const [selectedIds, setSelectedIds] = createSignal<string[]>([]);
  const [detailId, setDetailId] = createSignal<string | null>(null);
  const [inputMode, setInputMode] = createSignal<InputMode>("none");
  const [inputValue, setInputValue] = createSignal("");
  const [inputTargets, setInputTargets] = createSignal<string[]>([]);
  const [overlay, setOverlay] = createSignal<Overlay>(null);
  const [chordExpiresAt, setChordExpiresAt] = createSignal<number | null>(null);
  const [paletteQuery, setPaletteQuery] = createSignal("");
  const [paletteTodos, setPaletteTodos] = createSignal<Todo[]>([]);
  const [theme, setTheme] = createSignal<ThemePreference>(readPreference(localStorage));
  const undo = new UndoStack();
  const rowElements = new Map<string, HTMLButtonElement>();
  let editorInput: HTMLInputElement | undefined;
  let paletteInput: HTMLInputElement | undefined;
  let chordTimer: number | undefined;
  let paletteRequest = 0;

  const currentTodo = createMemo(() => todos()[cursorIndex()]);
  const detailTodo = createMemo(() =>
    todos().find((todo) => todo.id === detailId()),
  );
  const paletteItems = createMemo<readonly PaletteItem[]>(() => {
    const query = paletteQuery().trim().toLocaleLowerCase();
    const commands = COMMANDS.filter(
      (item) =>
        item.kind === "command" &&
        (query === "" || item.label.toLocaleLowerCase().includes(query)),
    );
    return [
      ...commands,
      ...paletteTodos().map((todo): PaletteItem => ({ kind: "todo", todo })),
    ];
  });

  function filterForCurrentView(): Filter {
    return {
      status: statusFilter(),
      q: searchQuery().trim() || undefined,
    };
  }

  function installTodos(next: Todo[], preferredId?: string): void {
    const activeId = preferredId ?? currentTodo()?.id;
    setTodos(next);
    setSelectedIds((selected) =>
      selected.filter((id) => next.some((todo) => todo.id === id)),
    );
    if (next.length === 0) {
      setCursorIndex(0);
      setDetailId(null);
      return;
    }
    const preserved = activeId === undefined ? -1 : next.findIndex((todo) => todo.id === activeId);
    setCursorIndex((index) =>
      preserved >= 0 ? preserved : Math.min(index, next.length - 1),
    );
    if (detailId() !== null && !next.some((todo) => todo.id === detailId())) {
      setDetailId(null);
    }
  }

  async function load(filter: Filter = filterForCurrentView(), preferredId?: string): Promise<Todo[]> {
    try {
      const next = await props.client.list(filter);
      installTodos(next, preferredId);
      setError(null);
      return next;
    } catch (reason) {
      setError(messageFrom(reason));
      return [];
    } finally {
      setLoading(false);
    }
  }

  function focusEditor(): void {
    queueMicrotask(() => {
      editorInput?.focus();
      editorInput?.select();
    });
  }

  function focusCurrentRow(): void {
    queueMicrotask(() => {
      const id = currentTodo()?.id;
      if (id !== undefined) {
        rowElements.get(id)?.focus();
      }
    });
  }

  function beginInput(mode: Exclude<InputMode, "none" | "palette" | "search">, value: string, ids: readonly string[]): void {
    setInputMode(mode);
    setInputValue(value);
    setInputTargets([...ids]);
    focusEditor();
  }

  function cancelInput(): void {
    if (inputMode() === "search") {
      setSearchQuery("");
      void load({ status: statusFilter() });
    }
    setInputMode("none");
    setInputValue("");
    setInputTargets([]);
    focusCurrentRow();
  }

  async function submitInput(keepCreating: boolean): Promise<void> {
    const mode = inputMode();
    const value = inputValue();
    try {
      if (mode === "create") {
        const created = await props.client.create({ title: value });
        undo.push([{ type: "Remove", id: created.id }]);
        setToast("Todo를 만들었습니다.");
        setInputValue("");
        if (!keepCreating) {
          setInputMode("none");
        }
        await load(filterForCurrentView(), created.id);
        if (keepCreating) {
          focusEditor();
        } else {
          focusCurrentRow();
        }
      } else if (mode === "edit") {
        const id = inputTargets()[0];
        if (id === undefined) return;
        await props.client.update(id, { title: value });
        setInputMode("none");
        await load(filterForCurrentView(), id);
        focusCurrentRow();
      } else if (mode === "due") {
        for (const id of inputTargets()) {
          const todo = todos().find((item) => item.id === id);
          if (todo === undefined) continue;
          await props.client.update(id, { due_date: value });
        }
        setInputMode("none");
        setToast("마감일을 바꿨습니다.");
        await load();
        focusCurrentRow();
      } else if (mode === "link") {
        const id = inputTargets()[0];
        if (id === undefined) return;
        await props.client.linkLinear(id, value);
        setInputMode("none");
        setToast("Linear 이슈를 연결했습니다.");
        focusCurrentRow();
      } else if (mode === "search") {
        setInputMode("none");
        focusCurrentRow();
      }
      setError(null);
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  async function toggleStatus(ids: readonly string[], kind: "done" | "in_progress"): Promise<void> {
    const inverses: InverseAction[] = [];
    try {
      for (const id of ids) {
        const todo = todos().find((item) => item.id === id);
        if (todo === undefined) continue;
        const next: Status = todo.status === kind ? "todo" : kind;
        await props.client.setStatus(id, next);
        inverses.push({ type: "SetStatus", id, status: todo.status });
      }
      undo.push(inverses);
      setError(null);
    } catch (reason) {
      undo.push(inverses);
      setError(messageFrom(reason));
    } finally {
      await load();
    }
  }

  async function setPriority(ids: readonly string[], priority: Priority): Promise<void> {
    const inverses: InverseAction[] = [];
    try {
      for (const id of ids) {
        const todo = todos().find((item) => item.id === id);
        if (todo === undefined) continue;
        await props.client.update(id, { priority });
        inverses.push({ type: "SetPriority", id, priority: todo.priority });
      }
      undo.push(inverses);
      setToast(`우선순위: ${PRIORITY_LABELS[priority]}`);
      setError(null);
    } catch (reason) {
      undo.push(inverses);
      setError(messageFrom(reason));
    } finally {
      await load();
    }
  }

  async function removeTodos(ids: readonly string[]): Promise<void> {
    const inverses: InverseAction[] = [];
    try {
      for (const id of ids) {
        await props.client.remove(id);
        inverses.push({ type: "Restore", id });
      }
      undo.push(inverses);
      setSelectedIds([]);
      setToast("삭제됨. u로 되돌리기");
      setError(null);
    } catch (reason) {
      undo.push(inverses);
      setError(messageFrom(reason));
    } finally {
      await load();
    }
  }

  async function applyUndo(): Promise<void> {
    const entry = undo.pop();
    if (entry === undefined) {
      setToast("되돌릴 동작이 없습니다.");
      return;
    }
    try {
      for (const inverse of entry) {
        switch (inverse.type) {
          case "SetStatus":
            await props.client.setStatus(inverse.id, inverse.status);
            break;
          case "SetPriority":
            await props.client.update(inverse.id, { priority: inverse.priority });
            break;
          case "Restore":
            await props.client.restore(inverse.id);
            break;
          case "Remove":
            await props.client.remove(inverse.id);
            break;
        }
      }
      setToast("되돌렸습니다.");
      setError(null);
    } catch (reason) {
      setError(messageFrom(reason));
    } finally {
      await load();
    }
  }

  function closeOverlay(): void {
    setOverlay(null);
    setInputMode("none");
    setPaletteQuery("");
    setPaletteTodos([]);
    focusCurrentRow();
  }

  function openPalette(): void {
    setOverlay("palette");
    setInputMode("palette");
    setPaletteQuery("");
    setPaletteTodos([]);
    queueMicrotask(() => paletteInput?.focus());
  }

  async function searchPalette(value: string): Promise<void> {
    setPaletteQuery(value);
    const request = ++paletteRequest;
    if (value.trim() === "") {
      setPaletteTodos([]);
      return;
    }
    try {
      const matches = await props.client.list({ q: value, limit: 20 });
      if (request === paletteRequest) {
        setPaletteTodos(matches);
        setError(null);
      }
    } catch (reason) {
      if (request === paletteRequest) {
        setError(messageFrom(reason));
      }
    }
  }

  async function choosePaletteItem(item = paletteItems()[0]): Promise<void> {
    if (item === undefined) return;
    if (item.kind === "todo") {
      closeOverlay();
      setStatusFilter(undefined);
      setSearchQuery("");
      const allTodos = await load({}, item.todo.id);
      const index = allTodos.findIndex((todo) => todo.id === item.todo.id);
      if (index >= 0) {
        setCursorIndex(index);
        focusCurrentRow();
      }
      return;
    }
    const command = item.id;
    closeOverlay();
    switch (command) {
      case "create":
        beginInput("create", "", []);
        break;
      case "help":
        setOverlay("help");
        break;
      case "theme":
        toggleTheme();
        break;
      case "pull-linear":
        try {
          const result = await props.client.pullLinear();
          setToast(`Linear: 새로 ${result.created}건, 건너뜀 ${result.skipped}건`);
          await load();
        } catch (reason) {
          setError(messageFrom(reason));
        }
        break;
    }
  }

  function toggleTheme(): void {
    const next = cyclePreference(theme());
    setTheme(next);
    applyPreference(document.documentElement, next);
    savePreference(localStorage, next);
    setToast(`테마: ${THEME_LABELS[next]}`);
  }

  function toggleSelection(id: string): void {
    setSelectedIds((selected) =>
      selected.includes(id)
        ? selected.filter((selectedId) => selectedId !== id)
        : [...selected, id],
    );
  }

  function scheduleChord(expiresAt: number): void {
    if (chordTimer !== undefined) window.clearTimeout(chordTimer);
    setChordExpiresAt(expiresAt);
    chordTimer = window.setTimeout(() => setChordExpiresAt(null), 500);
  }

  function changeFilter(status?: Status): void {
    setStatusFilter(status);
    setSelectedIds([]);
    setCursorIndex(0);
    void load({ status, q: searchQuery().trim() || undefined });
  }

  async function execute(action: Action): Promise<void> {
    switch (action.type) {
      case "MoveCursor":
        setCursorIndex(action.index);
        focusCurrentRow();
        break;
      case "OpenDetail":
        setDetailId(action.id);
        break;
      case "CloseDetail":
        setDetailId(null);
        focusCurrentRow();
        break;
      case "ClearSelection":
        setSelectedIds([]);
        break;
      case "CancelInput":
        cancelInput();
        break;
      case "SubmitInput":
        await submitInput(action.keepCreating);
        break;
      case "BeginCreate":
        beginInput("create", "", []);
        break;
      case "BeginEdit": {
        const todo = todos().find((item) => item.id === action.id);
        if (todo !== undefined) beginInput("edit", todo.title, [todo.id]);
        break;
      }
      case "ToggleDone":
        await toggleStatus(action.ids, "done");
        break;
      case "ToggleInProgress":
        await toggleStatus(action.ids, "in_progress");
        break;
      case "BeginPriorityChord":
        scheduleChord(action.expiresAt);
        setToast("우선순위: u h m l n");
        break;
      case "SetPriority":
        setChordExpiresAt(null);
        await setPriority(action.ids, action.priority);
        break;
      case "BeginDueDate":
        beginInput("due", "", action.ids);
        break;
      case "ToggleSelection":
        toggleSelection(action.id);
        break;
      case "Delete":
        await removeTodos(action.ids);
        break;
      case "BeginLinearLink":
        beginInput("link", "", [action.id]);
        break;
      case "OpenLinearIssue":
        setError("REST todo 응답에 Linear URL이 없어 M5 전에는 이 이슈를 열 수 없습니다.");
        break;
      case "SetFilter":
        changeFilter(action.status);
        break;
      case "Undo":
        await applyUndo();
        break;
      case "OpenPalette":
        openPalette();
        break;
      case "CloseOverlay":
        closeOverlay();
        break;
      case "OpenSearch":
        setInputMode("search");
        queueMicrotask(() => editorInput?.focus());
        break;
      case "OpenHelp":
        setOverlay("help");
        break;
      case "ChoosePaletteItem":
        await choosePaletteItem();
        break;
    }
  }

  function scopeStack(): ShortcutScope[] {
    const stack: ShortcutScope[] = ["global", "todo"];
    const currentOverlay = overlay();
    if (currentOverlay !== null) stack.push(currentOverlay);
    return stack;
  }

  function onKeyDown(event: KeyboardEvent): void {
    const shortcutEvent: ShortcutKeyEvent = {
      key: event.key,
      at: performance.now(),
      focus: isTextTarget(event.target) ? "text" : "other",
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      shiftKey: event.shiftKey,
    };
    const hadChord = chordExpiresAt() !== null;
    const action = handleKey(
      {
        scopeStack: scopeStack(),
        todoIds: todos().map((todo) => todo.id),
        cursorIndex: cursorIndex(),
        selectedIds: selectedIds(),
        inputMode: inputMode(),
        detailOpen: detailId() !== null,
        chordExpiresAt: chordExpiresAt(),
      },
      shortcutEvent,
    );
    if (hadChord) setChordExpiresAt(null);
    if (action !== null) {
      event.preventDefault();
      void execute(action);
    }
  }

  onMount(() => {
    void load();
    const unsubscribe = props.client.subscribe(() => void load());
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => {
      unsubscribe();
      window.removeEventListener("keydown", onKeyDown);
      if (chordTimer !== undefined) window.clearTimeout(chordTimer);
    });
  });

  return (
    <div class="app-shell">
      <header class="topbar">
        <div class="brand">local/todo</div>
        <div class="topbar-end">
          <div class="shortcut-hint"><kbd>⌘K</kbd> commands · <kbd>?</kbd> help</div>
          <button
            type="button"
            class="theme-toggle"
            aria-label={`테마 전환. 지금은 ${THEME_LABELS[theme()]}`}
            onClick={toggleTheme}
          >
            {THEME_LABELS[theme()]}
          </button>
        </div>
      </header>

      <nav class="tabbar" role="tablist" aria-label="앱 탭">
        <button type="button" role="tab" aria-selected="true">Todo</button>
      </nav>

      <main class="todo-view">
        <div class="filter-row" aria-label="상태 필터">
          <For each={[
            [undefined, "전체", "0"],
            ["todo", "todo", "1"],
            ["in_progress", "in progress", "2"],
            ["done", "done", "3"],
          ] as const}>
            {([status, label, key]) => (
              <button
                type="button"
                class="filter-chip"
                classList={{ active: statusFilter() === status }}
                aria-pressed={statusFilter() === status}
                onClick={() => changeFilter(status)}
              >
                {label}<kbd>{key}</kbd>
              </button>
            )}
          </For>
          <Show when={selectedIds().length > 0}>
            <span class="selection-count">{selectedIds().length} selected</span>
          </Show>
        </div>

        <Show when={inputMode() === "search"}>
          <div class="inline-editor search-editor">
            <label for="todo-search">Search</label>
            <input
              id="todo-search"
              ref={editorInput}
              value={searchQuery()}
              placeholder="title or description"
              onInput={(event) => {
                setSearchQuery(event.currentTarget.value);
                void load({
                  status: statusFilter(),
                  q: event.currentTarget.value.trim() || undefined,
                });
              }}
            />
            <kbd>Esc</kbd>
          </div>
        </Show>

        <Show when={inputMode() === "due" || inputMode() === "link"}>
          <div class="inline-editor action-editor">
            <label for="action-input">
              {inputMode() === "due" ? "Due date" : "Linear issue"}
            </label>
            <input
              id="action-input"
              ref={editorInput}
              value={inputValue()}
              placeholder={inputMode() === "due" ? "tomorrow, fri, 3d, or blank" : "PI-1234"}
              onInput={(event) => setInputValue(event.currentTarget.value)}
            />
            <kbd>Enter</kbd>
          </div>
        </Show>

        <Show when={props.externalServerError}>
          {(message) => <div class="error-banner" role="alert">{message()}</div>}
        </Show>

        <Show when={error()}>
          {(message) => <div class="error-banner" role="alert">{message()}</div>}
        </Show>

        <section class="list-section" aria-label="Todo 목록">
          <Show when={inputMode() === "create"}>
            <div class="todo-row inline-create">
              <span class="row-marker">+</span>
              <input
                ref={editorInput}
                aria-label="새 todo 제목"
                value={inputValue()}
                placeholder="새 todo 제목"
                onInput={(event) => setInputValue(event.currentTarget.value)}
              />
              <span class="row-help">Enter save · ⇧Enter keep going</span>
            </div>
          </Show>

          <Show when={!loading()} fallback={<div class="empty-state">불러오는 중…</div>}>
            <Show when={todos().length > 0} fallback={<div class="empty-state">표시할 todo가 없습니다. c로 만드십시오.</div>}>
              <div role="listbox" aria-multiselectable="true">
                <For each={todos()}>
                  {(todo, index) => (
                    <Show
                      when={inputMode() === "edit" && inputTargets()[0] === todo.id}
                      fallback={
                        <button
                          type="button"
                          role="option"
                          class="todo-row"
                          classList={{
                            current: cursorIndex() === index(),
                            selected: selectedIds().includes(todo.id),
                            completed: todo.status === "done",
                          }}
                          aria-current={cursorIndex() === index() ? "true" : undefined}
                          aria-selected={selectedIds().includes(todo.id)}
                          data-todo-id={todo.id}
                          ref={(element) => rowElements.set(todo.id, element)}
                          onClick={() => setCursorIndex(index())}
                          onDblClick={() => setDetailId(todo.id)}
                        >
                          <span class="row-marker" aria-hidden="true">
                            {selectedIds().includes(todo.id) ? "◆" : cursorIndex() === index() ? "›" : "·"}
                          </span>
                          <span class="todo-title">{todo.title}</span>
                          <span class={`priority priority-${todo.priority}`}>
                            {PRIORITY_LABELS[todo.priority]}
                          </span>
                          <Show when={todo.due_date}>
                            {(date) => <time datetime={date()}>{date()}</time>}
                          </Show>
                          <span class={`status status-${todo.status}`}>
                            {STATUS_LABELS[todo.status]}
                          </span>
                        </button>
                      }
                    >
                      <div class="todo-row inline-edit">
                        <span class="row-marker">›</span>
                        <input
                          ref={editorInput}
                          aria-label="todo 제목 편집"
                          value={inputValue()}
                          onInput={(event) => setInputValue(event.currentTarget.value)}
                        />
                        <kbd>Enter</kbd>
                      </div>
                    </Show>
                  )}
                </For>
              </div>
            </Show>
          </Show>
        </section>
      </main>

      <aside
        class="detail-panel"
        classList={{ open: detailTodo() !== undefined }}
        aria-hidden={detailTodo() === undefined}
        aria-labelledby="detail-heading"
      >
        <Show when={detailTodo()}>
          {(todo) => (
            <>
              <div class="panel-header">
                <span>DETAIL</span><kbd>Esc</kbd>
              </div>
              <h1 id="detail-heading">{todo().title}</h1>
              <p class="description">{todo().description || "설명이 없습니다."}</p>
              <dl>
                <div><dt>Status</dt><dd>{STATUS_LABELS[todo().status]}</dd></div>
                <div><dt>Priority</dt><dd>{PRIORITY_LABELS[todo().priority]}</dd></div>
                <div><dt>Due</dt><dd>{todo().due_date ?? "—"}</dd></div>
                <div><dt>Created</dt><dd>{todo().created_at}</dd></div>
                <div><dt>Updated</dt><dd>{todo().updated_at}</dd></div>
              </dl>
            </>
          )}
        </Show>
      </aside>

      <Show when={overlay() === "palette"}>
        <div class="overlay-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) closeOverlay();
        }}>
          <section class="palette" role="dialog" aria-modal="true" aria-label="커맨드 팔레트">
            <input
              ref={paletteInput}
              aria-label="명령 또는 todo 검색"
              value={paletteQuery()}
              placeholder="명령 또는 todo 검색"
              onInput={(event) => void searchPalette(event.currentTarget.value)}
            />
            <div class="palette-results" role="listbox">
              <For each={paletteItems()}>
                {(item, index) => (
                  <button
                    type="button"
                    role="option"
                    aria-selected={index() === 0}
                    classList={{ active: index() === 0 }}
                    onClick={() => void choosePaletteItem(item)}
                  >
                    <span>{item.kind === "command" ? item.label : item.todo.title}</span>
                    <small>
                      {item.kind === "command"
                        ? item.hint
                        : `${STATUS_LABELS[item.todo.status]} · ${PRIORITY_LABELS[item.todo.priority]}`}
                    </small>
                  </button>
                )}
              </For>
              <Show when={paletteItems().length === 0}>
                <div class="palette-empty">결과가 없습니다.</div>
              </Show>
            </div>
          </section>
        </div>
      </Show>

      <Show when={overlay() === "help"}>
        <div class="overlay-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget) closeOverlay();
        }}>
          <section class="help-dialog" role="dialog" aria-modal="true" aria-labelledby="shortcut-heading">
            <div class="panel-header"><span id="shortcut-heading">SHORTCUTS</span><kbd>Esc</kbd></div>
            <dl>
              <For each={SHORTCUTS}>
                {([key, label]) => <div><dt>{key}</dt><dd>{label}</dd></div>}
              </For>
            </dl>
          </section>
        </div>
      </Show>

      <Show when={toast()}>
        {(message) => <div class="toast" role="status">{message()}</div>}
      </Show>
    </div>
  );
};
