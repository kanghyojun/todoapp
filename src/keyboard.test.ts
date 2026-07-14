import { afterEach, describe, expect, it, vi } from "vitest";
import { handleKey, type KeyboardState, type KeyEvent } from "./keyboard";

function state(overrides: Partial<KeyboardState> = {}): KeyboardState {
  return {
    scopeStack: ["global", "todo"],
    todoIds: ["a", "b", "c"],
    cursorIndex: 1,
    selectedIds: [],
    inputMode: "none",
    detailOpen: false,
    priorityChordActive: false,
    ...overrides,
  };
}

function event(key: string, overrides: Partial<KeyEvent> = {}): KeyEvent {
  return { key, at: 1_000, focus: "other", ...overrides };
}

describe("keyboard handler", () => {
  afterEach(() => vi.useRealTimers());

  it("moves with j/k and clamps at both ends", () => {
    expect(handleKey(state({ cursorIndex: 2 }), event("j"))).toEqual({
      type: "MoveCursor",
      index: 2,
    });
    expect(handleKey(state({ cursorIndex: 0 }), event("k"))).toEqual({
      type: "MoveCursor",
      index: 0,
    });
    expect(handleKey(state(), event("j"))).toEqual({ type: "MoveCursor", index: 2 });
    expect(handleKey(state(), event("k"))).toEqual({ type: "MoveCursor", index: 0 });
  });

  it("emits done and in-progress toggles for the cursor", () => {
    expect(handleKey(state(), event("d"))).toEqual({ type: "ToggleDone", ids: ["b"] });
    expect(handleKey(state(), event("i"))).toEqual({
      type: "ToggleInProgress",
      ids: ["b"],
    });
  });

  it("resolves p then u and cancels on p then z", () => {
    const start = handleKey(state(), event("p"));
    expect(start).toEqual({ type: "BeginPriorityChord" });
    const chordState = state({ priorityChordActive: true });
    expect(handleKey(chordState, event("u"))).toEqual({
      type: "SetPriority",
      ids: ["b"],
      priority: "urgent",
    });
    // 조합 안의 엉뚱한 키는 조합만 취소하고 아무 일도 안 한다.
    expect(handleKey(chordState, event("z"))).toBeNull();
  });

  it("the chord never expires: u long after p is still Urgent, not Undo", () => {
    // p 를 누른 지 한참 뒤에 u 를 눌러도 Urgent 다. 만료가 없으니
    // 조용히 되돌리기로 새지 않는다. 이게 이 변경의 핵심이다.
    const chordState = state({ priorityChordActive: true });
    expect(handleKey(chordState, event("u", { at: 9_999_999 }))).toEqual({
      type: "SetPriority",
      ids: ["b"],
      priority: "urgent",
    });
    // 조합이 없을 때의 u 는 되돌리기다.
    expect(handleKey(state(), event("u"))).toEqual({ type: "Undo" });
  });

  it("kills shortcuts in text fields while preserving Escape and Enter", () => {
    const editing = state({ inputMode: "create" });
    expect(handleKey(editing, event("d", { focus: "text" }))).toBeNull();
    expect(handleKey(editing, event("Escape", { focus: "text" }))).toEqual({
      type: "CancelInput",
    });
    expect(handleKey(editing, event("Enter", { focus: "text" }))).toEqual({
      type: "SubmitInput",
      keepCreating: false,
    });
  });

  it("opens Cmd+K globally and suppresses Todo keys in the palette", () => {
    expect(handleKey(state(), event("k", { metaKey: true }))).toEqual({
      type: "OpenPalette",
    });
    expect(
      handleKey(
        state({ scopeStack: ["global", "todo", "palette"] }),
        event("d"),
      ),
    ).toBeNull();
    expect(
      handleKey(
        state({
          scopeStack: ["global", "todo", "palette"],
          inputMode: "palette",
        }),
        event("Escape", { focus: "text" }),
      ),
    ).toEqual({ type: "CloseOverlay" });
  });

  it("moves the palette highlight with Ctrl+N/P and arrows", () => {
    const palette = state({
      scopeStack: ["global", "todo", "palette"],
      inputMode: "palette",
    });
    expect(handleKey(palette, event("n", { focus: "text", ctrlKey: true }))).toEqual({
      type: "MovePaletteCursor",
      direction: "next",
    });
    expect(handleKey(palette, event("p", { focus: "text", ctrlKey: true }))).toEqual({
      type: "MovePaletteCursor",
      direction: "prev",
    });
    expect(handleKey(palette, event("ArrowDown", { focus: "text" }))).toEqual({
      type: "MovePaletteCursor",
      direction: "next",
    });
    expect(handleKey(palette, event("ArrowUp", { focus: "text" }))).toEqual({
      type: "MovePaletteCursor",
      direction: "prev",
    });
    // Ctrl 없는 맨 n/p 는 그냥 입력이라 아무 액션도 아니다.
    expect(handleKey(palette, event("n", { focus: "text" }))).toBeNull();
    // 팔레트가 아닌 인라인 입력에서는 Ctrl+N 이 항목 이동이 아니다.
    expect(
      handleKey(state({ inputMode: "create" }), event("n", { focus: "text", ctrlKey: true })),
    ).toBeNull();
  });

  it("targets every selected id for bulk actions", () => {
    expect(handleKey(state(), event("x"))).toEqual({
      type: "ToggleSelection",
      id: "b",
    });
    expect(
      handleKey(state({ selectedIds: ["a", "c"] }), event("d")),
    ).toEqual({ type: "ToggleDone", ids: ["a", "c"] });
  });
});
