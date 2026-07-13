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
import type { Filter, LinearStatus, Priority, Status, Todo } from "./domain";
import { MailView } from "./mail/MailView";
import type { GmailClient } from "./mail/client";
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
  gmailClient?: GmailClient;
  externalServerError?: string;
}

type Tab = "todo" | "mail";

type Overlay = "palette" | "help" | null;

type PaletteCommand =
  | "create"
  | "pull-linear"
  | "set-linear-key"
  | "help"
  | "theme"
  | "mail-view"
  | "mail-add-account"
  | "mail-sync";

type PaletteItem =
  | { kind: "command"; id: PaletteCommand; label: string; hint: string }
  | { kind: "todo"; todo: Todo };

// 화면에 보이는 상태 표기는 한국어. DB·REST·MCP 로 오가는 값은
// todo/in_progress/done/deferred 그대로다. 표시와 저장을 나눈다.
const STATUS_LABELS: Record<Status, string> = {
  todo: "할 일",
  in_progress: "진행 중",
  done: "완료",
  deferred: "보류",
};

const PRIORITY_LABELS: Record<Priority, string> = {
  none: "없음",
  urgent: "긴급",
  high: "높음",
  medium: "보통",
  low: "낮음",
};

interface CommandItem {
  kind: "command";
  id: PaletteCommand;
  label: string;
  hint: string;
}

const COMMANDS: readonly CommandItem[] = [
  { kind: "command", id: "create", label: "새 할 일", hint: "c" },
  { kind: "command", id: "pull-linear", label: "Linear 이슈 가져오기", hint: "" },
  { kind: "command", id: "set-linear-key", label: "Linear API 키 설정", hint: "" },
  { kind: "command", id: "theme", label: "테마 전환", hint: "" },
  { kind: "command", id: "help", label: "단축키 도움말", hint: "?" },
];

// 키가 없으면 Linear 관련 명령을 팔레트에서 숨긴다.
// 키를 아예 읽지 못하는 기기(key_store_available=false)면 설정 명령도 숨긴다.
function commandIsVisible(
  id: PaletteCommand,
  linear: LinearStatus | null,
): boolean {
  switch (id) {
    case "pull-linear":
      return linear?.configured === true;
    case "set-linear-key":
      return linear?.keyStoreAvailable === true;
    default:
      return true;
  }
}

const THEME_LABELS: Record<ThemePreference, string> = {
  auto: "자동",
  light: "밝게",
  dark: "어둡게",
};

// 인라인 입력창(due·link·linear_key·defer)의 라벨과 안내. 없는 모드는
// 인라인 편집(create·edit)이나 검색이라 여기 안 들어온다.
const ACTION_EDITOR: Partial<Record<InputMode, { label: string; placeholder: string }>> = {
  due: { label: "마감일", placeholder: "tomorrow, fri, 3d, 또는 비움" },
  link: { label: "Linear 이슈", placeholder: "PI-1234" },
  linear_key: { label: "Linear API key", placeholder: "lin_api_…" },
  defer: { label: "보류까지", placeholder: "next week, 3d, 또는 비우면 계속 보류" },
};

const SHORTCUTS: readonly [string, string][] = [
  ["j / k", "아래 / 위로 이동"],
  ["Enter / Esc", "상세 열기 / 닫기·취소·선택 해제"],
  ["c", "새 할 일. Shift+Enter로 연속 생성"],
  ["e", "제목 편집"],
  ["d / i", "완료 / 진행 중 토글"],
  ["s", "보류 토글 (복귀일 입력, 비우면 계속 보류)"],
  ["p u·h·m·l·n", "우선순위 지정"],
  ["t", "마감일 입력"],
  ["x", "선택 토글"],
  ["Backspace", "삭제"],
  ["l / o", "Linear 이슈 연결 / 열기"],
  ["1 / 2 / 3 / 4", "할 일 / 진행 중 / 완료 / 전체"],
  ["g", "보류 레인 펼치기·접기"],
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
  const [priorityChordActive, setPriorityChordActive] = createSignal(false);
  const [paletteQuery, setPaletteQuery] = createSignal("");
  const [paletteTodos, setPaletteTodos] = createSignal<Todo[]>([]);
  const [theme, setTheme] = createSignal<ThemePreference>(readPreference(localStorage));
  const [linearStatus, setLinearStatus] = createSignal<LinearStatus | null>(null);
  const [deferredTodos, setDeferredTodos] = createSignal<Todo[]>([]);
  const [laneOpen, setLaneOpen] = createSignal(localStorage.getItem("todo.lane") === "open");
  // Command 을 누르고 있으면 필터 칩 힌트를 ⌘1 처럼 보여준다.
  const [metaHeld, setMetaHeld] = createSignal(false);
  const [activeTab, setActiveTab] = createSignal<Tab>("todo");
  const undo = new UndoStack();
  const rowElements = new Map<string, HTMLButtonElement>();
  let editorInput: HTMLInputElement | undefined;
  let descEditor: HTMLTextAreaElement | undefined;
  let paletteInput: HTMLInputElement | undefined;
  let paletteRequest = 0;

  const currentTodo = createMemo(() => todos()[cursorIndex()]);
  const detailTodo = createMemo(() => {
    const id = detailId();
    return (
      todos().find((todo) => todo.id === id) ??
      deferredTodos().find((todo) => todo.id === id)
    );
  });
  const availableCommands = createMemo<readonly CommandItem[]>(() => {
    const list: CommandItem[] = [...COMMANDS];
    if (props.gmailClient) {
      list.push(
        { kind: "command", id: "mail-view", label: "메일 보기", hint: "" },
        { kind: "command", id: "mail-add-account", label: "메일 계정 추가", hint: "" },
        { kind: "command", id: "mail-sync", label: "메일 동기화", hint: "" },
      );
    }
    return list;
  });
  const paletteItems = createMemo<readonly PaletteItem[]>(() => {
    const query = paletteQuery().trim().toLocaleLowerCase();
    const status = linearStatus();
    const commands = availableCommands().filter(
      (item) =>
        commandIsVisible(item.id, status) &&
        (query === "" || item.label.toLocaleLowerCase().includes(query)),
    );
    return [
      ...commands,
      ...paletteTodos().map((todo): PaletteItem => ({ kind: "todo", todo })),
    ];
  });

  async function refreshLinearStatus(): Promise<void> {
    try {
      setLinearStatus(await props.client.linearStatus());
    } catch {
      // status 는 항상 200 이어야 한다. 실패해도 앱을 막지 않는다.
    }
  }

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
      void loadDeferred();
      return next;
    } catch (reason) {
      setError(messageFrom(reason));
      return [];
    } finally {
      setLoading(false);
    }
  }

  // 보류 레인은 별도 목록이다. 복귀일 빠른 순, 무기한(NULL)은 맨 뒤.
  async function loadDeferred(): Promise<void> {
    try {
      const deferred = await props.client.list({ status: "deferred" });
      deferred.sort((a, b) => {
        if (a.deferred_until === b.deferred_until) return 0;
        if (a.deferred_until === null) return 1;
        if (b.deferred_until === null) return -1;
        return a.deferred_until < b.deferred_until ? -1 : 1;
      });
      setDeferredTodos(deferred);
    } catch {
      // 레인은 부가 정보다. 실패해도 본 목록을 막지 않는다.
    }
  }

  function focusEditor(): void {
    queueMicrotask(() => {
      // 설명은 textarea 다. 기존 내용을 실수로 덮어쓰지 않게
      // 전체 선택 대신 커서만 끝으로 보낸다.
      if (inputMode() === "describe") {
        descEditor?.focus();
        const end = descEditor?.value.length ?? 0;
        descEditor?.setSelectionRange(end, end);
        return;
      }
      // 제목 같은 한 줄 입력은 전체 선택이 편하다.
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
        setToast("할 일을 만들었습니다.");
        setInputValue("");
        if (!keepCreating) {
          setInputMode("none");
        }
        await load(filterForCurrentView(), created.id);
        if (keepCreating) {
          focusEditor();
        } else {
          // 만들자마자 상세를 열고 설명 편집으로 넘어간다.
          setDetailId(created.id);
          beginInput("describe", created.description, [created.id]);
        }
      } else if (mode === "edit") {
        const id = inputTargets()[0];
        if (id === undefined) return;
        await props.client.update(id, { title: value });
        setInputMode("none");
        await load(filterForCurrentView(), id);
        focusCurrentRow();
      } else if (mode === "describe") {
        const id = inputTargets()[0];
        if (id === undefined) return;
        await props.client.update(id, { description: value });
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
        await load();
        focusCurrentRow();
      } else if (mode === "linear_key") {
        const key = value.trim();
        if (key === "") return;
        await props.client.setLinearKey(key);
        setInputMode("none");
        setInputValue("");
        setToast("Linear 키를 저장했습니다.");
        await refreshLinearStatus();
        focusCurrentRow();
      } else if (mode === "defer") {
        for (const id of inputTargets()) {
          await props.client.defer(id, value);
        }
        setInputMode("none");
        setToast(value.trim() === "" ? "계속 보류합니다." : "보류했습니다.");
        await load();
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

  // s: 대상이 보류면 바로 풀고(todo 로), 아니면 복귀일 입력창을 연다.
  async function toggleDefer(ids: readonly string[]): Promise<void> {
    const deferredIds = ids.filter((id) =>
      deferredTodos().some((todo) => todo.id === id),
    );
    if (deferredIds.length > 0) {
      await bringBack(deferredIds);
      return;
    }
    beginInput("defer", "", ids);
  }

  // 보류를 풀어 todo 로 되돌린다. status 를 바꾸면 코어가 복귀일도 지운다.
  async function bringBack(ids: readonly string[]): Promise<void> {
    try {
      for (const id of ids) {
        await props.client.setStatus(id, "todo");
      }
      setToast("보류를 풀었습니다.");
      await load();
    } catch (reason) {
      setError(messageFrom(reason));
    }
  }

  function toggleLane(): void {
    const next = !laneOpen();
    setLaneOpen(next);
    localStorage.setItem("todo.lane", next ? "open" : "closed");
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
      case "set-linear-key":
        beginInput("linear_key", "", []);
        break;
      case "pull-linear":
        try {
          const result = await props.client.pullLinear();
          setToast(`Linear: 새로 ${result.created}건, 건너뜀 ${result.skipped}건`);
          await load();
          await refreshLinearStatus();
        } catch (reason) {
          setError(messageFrom(reason));
        }
        break;
      case "mail-view":
        setActiveTab("mail");
        break;
      case "mail-add-account":
        if (props.gmailClient) {
          try {
            await props.gmailClient.addAccount();
            setActiveTab("mail");
          } catch (reason) {
            setError(messageFrom(reason));
          }
        }
        break;
      case "mail-sync":
        if (props.gmailClient) {
          try {
            await props.gmailClient.sync();
            setToast("메일 동기화를 시작했습니다.");
          } catch (reason) {
            setError(messageFrom(reason));
          }
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
      case "BeginEditDescription": {
        const todo =
          todos().find((item) => item.id === action.id) ??
          deferredTodos().find((item) => item.id === action.id);
        if (todo !== undefined) {
          setDetailId(todo.id);
          beginInput("describe", todo.description, [todo.id]);
        }
        break;
      }
      case "ToggleDone":
        await toggleStatus(action.ids, "done");
        break;
      case "ToggleInProgress":
        await toggleStatus(action.ids, "in_progress");
        break;
      case "ToggleDefer":
        await toggleDefer(action.ids);
        break;
      case "ToggleDeferredLane":
        toggleLane();
        break;
      case "BeginPriorityChord":
        setPriorityChordActive(true);
        setToast("우선순위: u h m l n (Esc 취소)");
        break;
      case "SetPriority":
        setPriorityChordActive(false);
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
        if (linearStatus()?.configured !== true) {
          setToast("Linear API 키를 먼저 설정하십시오. ⌘K → Set Linear API key");
          break;
        }
        beginInput("link", "", [action.id]);
        break;
      case "OpenLinearIssue": {
        const linked = todos().find((todo) => todo.id === action.id);
        if (linked?.linear) {
          try {
            await props.client.openExternal(linked.linear.url);
          } catch (reason) {
            setError(messageFrom(reason));
          }
        } else {
          setToast("연결된 Linear 이슈가 없습니다.");
        }
        break;
      }
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
    const base: ShortcutScope = activeTab() === "mail" ? "mail" : "todo";
    const stack: ShortcutScope[] = ["global", base];
    const currentOverlay = overlay();
    if (currentOverlay !== null) stack.push(currentOverlay);
    return stack;
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.metaKey || event.key === "Meta") setMetaHeld(true);
    const shortcutEvent: ShortcutKeyEvent = {
      key: event.key,
      at: performance.now(),
      focus: isTextTarget(event.target) ? "text" : "other",
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      shiftKey: event.shiftKey,
    };
    // 조합이 열려 있었으면 이 키로 닫는다. 우선순위면 SetPriority 로
    // 소비되고, 아니면 그냥 닫히기만 한다. 만료 타이머는 없다.
    const hadChord = priorityChordActive();
    const action = handleKey(
      {
        scopeStack: scopeStack(),
        todoIds: todos().map((todo) => todo.id),
        cursorIndex: cursorIndex(),
        selectedIds: selectedIds(),
        inputMode: inputMode(),
        detailOpen: detailId() !== null,
        priorityChordActive: priorityChordActive(),
      },
      shortcutEvent,
    );
    if (hadChord) setPriorityChordActive(false);
    if (action !== null) {
      event.preventDefault();
      void execute(action);
    }
  }

  function onKeyUp(event: KeyboardEvent): void {
    if (event.key === "Meta" || !event.metaKey) setMetaHeld(false);
  }
  // 창을 벗어나면 keyup 을 놓칠 수 있으니 ⌘ 힌트를 확실히 끈다.
  function clearMeta(): void {
    setMetaHeld(false);
  }

  onMount(() => {
    void load();
    void refreshLinearStatus();
    const unsubscribe = props.client.subscribe(() => void load());
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", clearMeta);
    onCleanup(() => {
      unsubscribe();
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", clearMeta);
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
        <button
          type="button"
          role="tab"
          aria-selected={activeTab() === "todo"}
          classList={{ active: activeTab() === "todo" }}
          onClick={() => setActiveTab("todo")}
        >
          Todo
        </button>
        <Show when={props.gmailClient}>
          <button
            type="button"
            role="tab"
            aria-selected={activeTab() === "mail"}
            classList={{ active: activeTab() === "mail" }}
            onClick={() => setActiveTab("mail")}
          >
            Mail
          </button>
        </Show>
      </nav>

      <Show when={activeTab() === "mail" ? props.gmailClient : undefined}>
        {(client) => <MailView client={client()} />}
      </Show>

      <Show when={activeTab() === "todo"}>
      <main class="todo-view">
        <div class="filter-row" aria-label="상태 필터">
          <For each={[
            ["todo", "할 일", "1"],
            ["in_progress", "진행 중", "2"],
            ["done", "완료", "3"],
            [undefined, "전체", "4"],
          ] as const}>
            {([status, label, key]) => (
              <button
                type="button"
                class="filter-chip"
                classList={{ active: statusFilter() === status }}
                aria-pressed={statusFilter() === status}
                onClick={() => changeFilter(status)}
              >
                {label}<kbd>{metaHeld() ? `⌘${key}` : key}</kbd>
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

        <Show when={ACTION_EDITOR[inputMode()] !== undefined}>
          {(() => {
            const editor = () => ACTION_EDITOR[inputMode()];
            return (
              <div class="inline-editor action-editor">
                <label for="action-input">{editor()?.label}</label>
                <input
                  id="action-input"
                  ref={editorInput}
                  type={inputMode() === "linear_key" ? "password" : "text"}
                  autocomplete={inputMode() === "linear_key" ? "off" : undefined}
                  value={inputValue()}
                  placeholder={editor()?.placeholder}
                  onInput={(event) => setInputValue(event.currentTarget.value)}
                />
                <kbd>Enter</kbd>
              </div>
            );
          })()}
        </Show>

        <Show when={props.externalServerError}>
          {(message) => <div class="error-banner" role="alert">{message()}</div>}
        </Show>

        <Show when={error()}>
          {(message) => <div class="error-banner" role="alert">{message()}</div>}
        </Show>

        <section class="list-section" aria-label="할 일 목록">
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
            <Show when={todos().length > 0} fallback={<div class="empty-state">표시할 할 일이 없습니다. c로 만드십시오.</div>}>
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

        <Show when={deferredTodos().length > 0}>
          <section class="lane" aria-label="보류">
            <button
              type="button"
              class="lane-header"
              aria-expanded={laneOpen()}
              onClick={toggleLane}
            >
              <span class="lane-caret">{laneOpen() ? "▾" : "▸"}</span>
              보류 {deferredTodos().length}
              <kbd>g</kbd>
            </button>
            <Show when={laneOpen()}>
              <div class="lane-items">
                <For each={deferredTodos()}>
                  {(todo) => (
                    <div class="lane-row">
                      <button
                        type="button"
                        class="lane-title"
                        onClick={() => setDetailId(todo.id)}
                      >
                        {todo.title}
                      </button>
                      <span class="lane-until">
                        {todo.deferred_until ?? "계속 보류"}
                      </span>
                      <button
                        type="button"
                        class="lane-back"
                        onClick={() => void bringBack([todo.id])}
                      >
                        복귀
                      </button>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </section>
        </Show>
      </main>
      </Show>

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
              <div class="detail-body">
              <Show
                when={inputMode() === "describe" && inputTargets()[0] === todo().id}
                fallback={
                  <p
                    class="description"
                    onClick={() => void execute({ type: "BeginEditDescription", id: todo().id })}
                    title="눌러서 편집 (E)"
                  >
                    {todo().description || "설명이 없습니다."}
                  </p>
                }
              >
                <textarea
                  ref={descEditor}
                  class="description-editor"
                  value={inputValue()}
                  onInput={(event) => setInputValue(event.currentTarget.value)}
                  placeholder="설명을 적으세요. ⌘Enter 저장, Esc 취소"
                />
              </Show>
              <dl>
                <div><dt>상태</dt><dd>{STATUS_LABELS[todo().status]}</dd></div>
                <div><dt>우선순위</dt><dd>{PRIORITY_LABELS[todo().priority]}</dd></div>
                <div><dt>마감</dt><dd>{todo().due_date ?? "—"}</dd></div>
                <Show when={todo().status === "deferred"}>
                  <div><dt>복귀</dt><dd>{todo().deferred_until ?? "계속 보류"}</dd></div>
                </Show>
                <div><dt>만든 때</dt><dd>{todo().created_at}</dd></div>
                <div><dt>고친 때</dt><dd>{todo().updated_at}</dd></div>
                <Show when={todo().linear}>
                  {(linear) => (
                    <div>
                      <dt>Linear</dt>
                      <dd>
                        <button
                          type="button"
                          class="linear-link"
                          onClick={() => void props.client.openExternal(linear().url)}
                        >
                          {linear().identifier}
                        </button>
                        <span class="row-help"> · o로 열기</span>
                      </dd>
                    </div>
                  )}
                </Show>
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
              aria-label="명령 또는 할 일 검색"
              value={paletteQuery()}
              placeholder="명령 또는 할 일 검색"
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
