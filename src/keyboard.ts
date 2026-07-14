import type { Priority, Status } from "./domain";

export type ShortcutScope = "global" | "todo" | "mail" | "palette" | "help";
export type InputMode =
  | "none"
  | "create"
  | "edit"
  | "describe"
  | "due"
  | "link"
  | "search"
  | "palette"
  | "linear_key"
  | "defer";

export interface KeyboardState {
  scopeStack: readonly ShortcutScope[];
  todoIds: readonly string[];
  cursorIndex: number;
  selectedIds: readonly string[];
  inputMode: InputMode;
  detailOpen: boolean;
  // p 를 눌러 우선순위 조합을 기다리는 중이다. 만료는 없다.
  // 다음 키가 우선순위면 적용하고, 아니면 조합만 취소한다.
  priorityChordActive: boolean;
}

export interface KeyEvent {
  key: string;
  at: number;
  focus: "text" | "other";
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
}

export type Action =
  | { type: "MoveCursor"; index: number }
  | { type: "OpenDetail"; id: string }
  | { type: "CloseDetail" }
  | { type: "ClearSelection" }
  | { type: "CancelInput" }
  | { type: "SubmitInput"; keepCreating: boolean }
  | { type: "BeginCreate" }
  | { type: "BeginEdit"; id: string }
  | { type: "BeginEditDescription"; id: string }
  | { type: "ToggleDone"; ids: readonly string[] }
  | { type: "ToggleInProgress"; ids: readonly string[] }
  | { type: "ToggleDefer"; ids: readonly string[] }
  | { type: "ToggleDeferredLane" }
  | { type: "BeginPriorityChord" }
  | { type: "SetPriority"; ids: readonly string[]; priority: Priority }
  | { type: "BeginDueDate"; ids: readonly string[] }
  | { type: "ToggleSelection"; id: string }
  | { type: "Delete"; ids: readonly string[] }
  | { type: "BeginLinearLink"; id: string }
  | { type: "OpenLink"; id: string }
  | { type: "SetFilter"; status?: Status }
  | { type: "SwitchTab"; direction: "prev" | "next" }
  | { type: "Undo" }
  | { type: "OpenPalette" }
  | { type: "CloseOverlay" }
  | { type: "OpenSearch" }
  | { type: "OpenHelp" }
  | { type: "MovePaletteCursor"; direction: "next" | "prev" }
  | { type: "ChoosePaletteItem" };

const PRIORITY_KEYS: Readonly<Record<string, Priority>> = {
  u: "urgent",
  h: "high",
  m: "medium",
  l: "low",
  n: "none",
};

function currentId(state: KeyboardState): string | null {
  return state.todoIds[state.cursorIndex] ?? null;
}

function targets(state: KeyboardState): readonly string[] {
  if (state.selectedIds.length > 0) {
    return state.selectedIds;
  }
  const id = currentId(state);
  return id === null ? [] : [id];
}

function escapeAction(state: KeyboardState): Action | null {
  const topScope = state.scopeStack.at(-1);
  if (topScope === "palette" || topScope === "help") {
    return { type: "CloseOverlay" };
  }
  if (state.inputMode !== "none") {
    return { type: "CancelInput" };
  }
  if (state.detailOpen) {
    return { type: "CloseDetail" };
  }
  if (state.selectedIds.length > 0) {
    return { type: "ClearSelection" };
  }
  return null;
}

export function handleKey(state: KeyboardState, event: KeyEvent): Action | null {
  if (event.key === "Escape") {
    return escapeAction(state);
  }
  if (event.focus === "text") {
    // 팔레트 입력창은 포커스가 잡혀 있어도 Ctrl+N/P(와 ↓/↑)로 목록을 넘긴다.
    // 한 줄 입력이라 이 키들의 기본 동작은 어차피 아무 일도 안 한다.
    if (state.inputMode === "palette") {
      const lower = event.key.toLowerCase();
      if (event.key === "ArrowDown" || (event.ctrlKey === true && lower === "n")) {
        return { type: "MovePaletteCursor", direction: "next" };
      }
      if (event.key === "ArrowUp" || (event.ctrlKey === true && lower === "p")) {
        return { type: "MovePaletteCursor", direction: "prev" };
      }
    }
    if (event.key === "Enter") {
      if (state.inputMode === "palette") {
        return { type: "ChoosePaletteItem" };
      }
      // 설명은 여러 줄이다. 맨 Enter 는 줄바꿈으로 흘려보내고
      // ⌘/Ctrl+Enter 로만 저장한다.
      if (state.inputMode === "describe") {
        return event.metaKey === true || event.ctrlKey === true
          ? { type: "SubmitInput", keepCreating: false }
          : null;
      }
      return {
        type: "SubmitInput",
        keepCreating: state.inputMode === "create" && event.shiftKey === true,
      };
    }
    return null;
  }

  const topScope = state.scopeStack.at(-1);
  const commandPalette =
    event.key.toLowerCase() === "k" &&
    (event.metaKey === true || event.ctrlKey === true);
  if (commandPalette) {
    return topScope === "palette"
      ? { type: "CloseOverlay" }
      : { type: "OpenPalette" };
  }
  if (topScope === "palette" || topScope === "help") {
    return null;
  }
  if (event.key === "?") {
    return { type: "OpenHelp" };
  }
  // 탭 전환은 어느 탭에서든 되는 전역 동작이다. [ 이전, ] 다음.
  if (event.key === "[") {
    return { type: "SwitchTab", direction: "prev" };
  }
  if (event.key === "]") {
    return { type: "SwitchTab", direction: "next" };
  }
  if (topScope !== "todo") {
    return null;
  }

  // 필터 전환은 ⌘/Ctrl + 숫자로만. 맨손 1~4 는 무시한다.
  // (칩에는 개수를 보여주고, ⌘ 를 눌렀을 때만 단축키 힌트를 띄운다.)
  if (event.metaKey === true || event.ctrlKey === true) {
    switch (event.key) {
      case "1":
        return { type: "SetFilter", status: "todo" };
      case "2":
        return { type: "SetFilter", status: "in_progress" };
      case "3":
        return { type: "SetFilter", status: "done" };
      case "4":
        return { type: "SetFilter" };
    }
  }

  // p 로 조합을 연 뒤에는 다음 키를 기다린다. 만료가 없으니
  // 시간이 얼마가 지나든 그다음 키 하나가 조합을 결정한다.
  // vim 이 d 를 누른 뒤 하는 것과 같다. 우선순위면 적용하고,
  // 아니면(Esc 포함) 조합만 취소하고 아무 일도 하지 않는다.
  if (state.priorityChordActive) {
    const priority = PRIORITY_KEYS[event.key.toLowerCase()];
    if (priority !== undefined) {
      const ids = targets(state);
      return ids.length === 0 ? null : { type: "SetPriority", ids, priority };
    }
    return null;
  }

  const id = currentId(state);
  const ids = targets(state);
  switch (event.key) {
    case "j":
      return state.todoIds.length === 0
        ? null
        : {
            type: "MoveCursor",
            index: Math.min(state.cursorIndex + 1, state.todoIds.length - 1),
          };
    case "k":
      return state.todoIds.length === 0
        ? null
        : { type: "MoveCursor", index: Math.max(state.cursorIndex - 1, 0) };
    case "Enter":
      return id === null ? null : { type: "OpenDetail", id };
    case "c":
      return { type: "BeginCreate" };
    case "e":
      return id === null ? null : { type: "BeginEdit", id };
    case "E":
      return id === null ? null : { type: "BeginEditDescription", id };
    case "d":
      return ids.length === 0 ? null : { type: "ToggleDone", ids };
    case "i":
      return ids.length === 0 ? null : { type: "ToggleInProgress", ids };
    case "s":
      return ids.length === 0 ? null : { type: "ToggleDefer", ids };
    case "g":
      return { type: "ToggleDeferredLane" };
    case "p":
      return ids.length === 0 ? null : { type: "BeginPriorityChord" };
    case "t":
      return ids.length === 0 ? null : { type: "BeginDueDate", ids };
    case "x":
      return id === null ? null : { type: "ToggleSelection", id };
    case "Backspace":
      return ids.length === 0 ? null : { type: "Delete", ids };
    case "l":
      return id === null ? null : { type: "BeginLinearLink", id };
    case "o":
      return id === null ? null : { type: "OpenLink", id };
    case "/":
      return { type: "OpenSearch" };
    case "u":
      return { type: "Undo" };
    default:
      return null;
  }
}
