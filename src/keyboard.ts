import type { Priority, Status } from "./domain";

export type ShortcutScope = "global" | "todo" | "palette" | "help";
export type InputMode =
  | "none"
  | "create"
  | "edit"
  | "due"
  | "link"
  | "search"
  | "palette"
  | "linear_key";

export interface KeyboardState {
  scopeStack: readonly ShortcutScope[];
  todoIds: readonly string[];
  cursorIndex: number;
  selectedIds: readonly string[];
  inputMode: InputMode;
  detailOpen: boolean;
  chordExpiresAt: number | null;
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
  | { type: "ToggleDone"; ids: readonly string[] }
  | { type: "ToggleInProgress"; ids: readonly string[] }
  | { type: "BeginPriorityChord"; expiresAt: number }
  | { type: "SetPriority"; ids: readonly string[]; priority: Priority }
  | { type: "BeginDueDate"; ids: readonly string[] }
  | { type: "ToggleSelection"; id: string }
  | { type: "Delete"; ids: readonly string[] }
  | { type: "BeginLinearLink"; id: string }
  | { type: "OpenLinearIssue"; id: string }
  | { type: "SetFilter"; status?: Status }
  | { type: "Undo" }
  | { type: "OpenPalette" }
  | { type: "CloseOverlay" }
  | { type: "OpenSearch" }
  | { type: "OpenHelp" }
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
    if (event.key === "Enter") {
      if (state.inputMode === "palette") {
        return { type: "ChoosePaletteItem" };
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
  if (event.key === "/") {
    return { type: "OpenSearch" };
  }
  if (topScope !== "todo") {
    return null;
  }

  // 만료된 chord 는 없는 것과 같다. 키를 삼키지 않고 평소대로 처리한다.
  // 그래야 p 를 눌렀다 마음이 바뀌어 u(되돌리기)를 눌러도 한 번에 먹는다.
  const chordActive = state.chordExpiresAt !== null && event.at < state.chordExpiresAt;
  if (chordActive) {
    const priority = PRIORITY_KEYS[event.key.toLowerCase()];
    if (priority !== undefined) {
      const ids = targets(state);
      return ids.length === 0 ? null : { type: "SetPriority", ids, priority };
    }
    // chord 창 안의 엉뚱한 키는 chord 만 취소하고 아무 일도 하지 않는다.
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
    case "d":
      return ids.length === 0 ? null : { type: "ToggleDone", ids };
    case "i":
      return ids.length === 0 ? null : { type: "ToggleInProgress", ids };
    case "p":
      return ids.length === 0
        ? null
        : { type: "BeginPriorityChord", expiresAt: event.at + 500 };
    case "t":
      return ids.length === 0 ? null : { type: "BeginDueDate", ids };
    case "x":
      return id === null ? null : { type: "ToggleSelection", id };
    case "Backspace":
      return ids.length === 0 ? null : { type: "Delete", ids };
    case "l":
      return id === null ? null : { type: "BeginLinearLink", id };
    case "o":
      return id === null ? null : { type: "OpenLinearIssue", id };
    case "1":
      return { type: "SetFilter", status: "todo" };
    case "2":
      return { type: "SetFilter", status: "in_progress" };
    case "3":
      return { type: "SetFilter", status: "done" };
    case "0":
      return { type: "SetFilter" };
    case "u":
      return { type: "Undo" };
    default:
      return null;
  }
}
