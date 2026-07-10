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
    chordExpiresAt: null,
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

  it("resolves p then u and ignores p then z", () => {
    const start = handleKey(state(), event("p"));
    expect(start).toEqual({ type: "BeginPriorityChord", expiresAt: 1_500 });
    const chordState = state({ chordExpiresAt: 1_500 });
    expect(handleKey(chordState, event("u", { at: 1_499 }))).toEqual({
      type: "SetPriority",
      ids: ["b"],
      priority: "urgent",
    });
    expect(handleKey(chordState, event("z", { at: 1_200 }))).toBeNull();
  });

  it("expires the priority chord after 500 ms", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const start = handleKey(state(), event("p", { at: Date.now() }));
    expect(start?.type).toBe("BeginPriorityChord");
    vi.advanceTimersByTime(500);

    // 만료된 chord 는 우선순위를 바꾸지 않는다. 대신 키를 삼키지도 않는다.
    // p 를 눌렀다 마음이 바뀌어 u(되돌리기)를 누르면 한 번에 먹어야 한다.
    expect(
      handleKey(state({ chordExpiresAt: 1_500 }), event("u", { at: Date.now() })),
    ).toEqual({ type: "Undo" });
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
