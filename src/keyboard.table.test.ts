import { describe, expect, it } from "vitest";
import { handleKey, type KeyboardState, type KeyEvent, type Action } from "./keyboard";

const base: KeyboardState = {
  scopeStack: ["global", "todo"],
  todoIds: ["a", "b", "c"],
  cursorIndex: 1,
  selectedIds: [],
  inputMode: "none",
  detailOpen: false,
  chordExpiresAt: null,
};

const press = (key: string, s: Partial<KeyboardState> = {}, e: Partial<KeyEvent> = {}): Action | null =>
  handleKey({ ...base, ...s }, { key, at: 1000, focus: "other", ...e });

describe("스펙 6절 단축키 표의 모든 행", () => {
  it("이동/열기/닫기", () => {
    expect(press("j")).toEqual({ type: "MoveCursor", index: 2 });
    expect(press("k")).toEqual({ type: "MoveCursor", index: 0 });
    expect(press("Enter")).toEqual({ type: "OpenDetail", id: "b" });
    expect(press("Escape", { detailOpen: true })).toEqual({ type: "CloseDetail" });
  });

  it("생성/편집", () => {
    expect(press("c")).toEqual({ type: "BeginCreate" });
    expect(press("e")).toEqual({ type: "BeginEdit", id: "b" });
  });

  it("상태 토글", () => {
    expect(press("d")).toEqual({ type: "ToggleDone", ids: ["b"] });
    expect(press("i")).toEqual({ type: "ToggleInProgress", ids: ["b"] });
  });

  it("우선순위 코드 5개 전부", () => {
    const chord = { chordExpiresAt: 1400 };
    for (const [key, priority] of [["u","urgent"],["h","high"],["m","medium"],["l","low"],["n","none"]] as const) {
      expect(press(key, chord)).toEqual({ type: "SetPriority", ids: ["b"], priority });
    }
  });

  it("마감일 / 선택 / 삭제", () => {
    expect(press("t")).toEqual({ type: "BeginDueDate", ids: ["b"] });
    expect(press("x")).toEqual({ type: "ToggleSelection", id: "b" });
    expect(press("Backspace")).toEqual({ type: "Delete", ids: ["b"] });
  });

  it("Linear 링크 / 열기", () => {
    expect(press("l")).toEqual({ type: "BeginLinearLink", id: "b" });
    expect(press("o")).toEqual({ type: "OpenLinearIssue", id: "b" });
  });

  it("필터 1/2/3/0", () => {
    expect(press("1")).toEqual({ type: "SetFilter", status: "todo" });
    expect(press("2")).toEqual({ type: "SetFilter", status: "in_progress" });
    expect(press("3")).toEqual({ type: "SetFilter", status: "done" });
    expect(press("0")).toEqual({ type: "SetFilter" });
  });

  it("검색 / 되돌리기 / 팔레트 / 도움말", () => {
    expect(press("/")).toEqual({ type: "OpenSearch" });
    expect(press("u")).toEqual({ type: "Undo" });
    expect(press("k", {}, { metaKey: true })).toEqual({ type: "OpenPalette" });
    expect(press("k", {}, { ctrlKey: true })).toEqual({ type: "OpenPalette" });
    expect(press("?")).toEqual({ type: "OpenHelp" });
  });
});

describe("함정들", () => {
  it("p 혼자서는 절대 우선순위를 안 바꾼다", () => {
    const a = press("p");
    expect(a?.type).toBe("BeginPriorityChord");
  });

  it("chord 만료 후 u 는 Undo 이지 Urgent 가 아니다", () => {
    // 만료된 chord (expiresAt 이 지났다)
    const a = handleKey({ ...base, chordExpiresAt: 900 }, { key: "u", at: 1000, focus: "other" });
    expect(a).toEqual({ type: "Undo" });
  });

  it("chord 유효 창 안에서는 u 가 Urgent", () => {
    const a = handleKey({ ...base, chordExpiresAt: 1400 }, { key: "u", at: 1000, focus: "other" });
    expect(a).toEqual({ type: "SetPriority", ids: ["b"], priority: "urgent" });
  });

  it("입력창 포커스면 단일키 전부 죽고 Esc/Enter 만 산다", () => {
    for (const key of ["j","k","d","i","p","t","x","c","e","l","o","u","1","0","/","?","Backspace"]) {
      expect(press(key, { inputMode: "create" }, { focus: "text" })).toBeNull();
    }
    expect(press("Escape", { inputMode: "create" }, { focus: "text" })).toEqual({ type: "CancelInput" });
    expect(press("Enter", { inputMode: "create" }, { focus: "text" })?.type).toBe("SubmitInput");
  });

  it("Shift+Enter 는 연속 입력", () => {
    const a = press("Enter", { inputMode: "create" }, { focus: "text", shiftKey: true });
    expect(a).toEqual({ type: "SubmitInput", keepCreating: true });
  });

  it("팔레트가 열려 있으면 todo 키가 안 먹는다", () => {
    const s = { scopeStack: ["global","todo","palette"] as const, inputMode: "palette" as const };
    expect(press("d", s, { focus: "text" })).toBeNull();
    expect(press("j", s, { focus: "text" })).toBeNull();
  });

  it("x 로 여러 개 고르면 d/t/Backspace 가 전부에 적용된다", () => {
    const s = { selectedIds: ["a","c"] };
    expect(press("d", s)).toEqual({ type: "ToggleDone", ids: ["a","c"] });
    expect(press("t", s)).toEqual({ type: "BeginDueDate", ids: ["a","c"] });
    expect(press("Backspace", s)).toEqual({ type: "Delete", ids: ["a","c"] });
  });

  it("커서가 끝에서 안 넘어간다", () => {
    expect(press("j", { cursorIndex: 2 })).toEqual({ type: "MoveCursor", index: 2 });
    expect(press("k", { cursorIndex: 0 })).toEqual({ type: "MoveCursor", index: 0 });
  });

  it("빈 목록에서 d 를 눌러도 터지지 않는다", () => {
    expect(press("d", { todoIds: [], cursorIndex: 0 })).toBeNull();
  });
});
