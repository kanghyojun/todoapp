import { describe, expect, it } from "vitest";
import { handleMailKey } from "./keyboard-mail";

const other = { focus: "other" as const, detailOpen: false, meta: false };
const meta = { focus: "other" as const, detailOpen: false, meta: true };

describe("handleMailKey", () => {
  it("j는 아래로 이동", () =>
    expect(handleMailKey(other, "j")).toEqual({ type: "Move", delta: 1 }));
  it("k는 위로 이동", () =>
    expect(handleMailKey(other, "k")).toEqual({ type: "Move", delta: -1 }));
  it("Enter는 열기", () =>
    expect(handleMailKey(other, "Enter")).toEqual({ type: "Open" }));
  it("e는 보관", () =>
    expect(handleMailKey(other, "e")).toEqual({ type: "Archive" }));
  it("u는 읽음 토글", () =>
    expect(handleMailKey(other, "u")).toEqual({ type: "ToggleRead" }));
  it("t는 현재 메일로 할 일 만들기", () =>
    expect(handleMailKey(other, "t")).toEqual({ type: "CreateTodo" }));
  it("a는 계정 순환", () =>
    expect(handleMailKey(other, "a")).toEqual({ type: "CycleAccount" }));
  it("⌘1/2/3은 폴더 전환", () => {
    expect(handleMailKey(meta, "1")).toEqual({ type: "SetFolder", folder: "inbox" });
    expect(handleMailKey(meta, "2")).toEqual({ type: "SetFolder", folder: "archive" });
    expect(handleMailKey(meta, "3")).toEqual({ type: "SetFolder", folder: "all" });
  });
  it("맨손 숫자는 폴더를 바꾸지 않는다", () => {
    expect(handleMailKey(other, "1")).toBeNull();
    expect(handleMailKey(other, "2")).toBeNull();
    expect(handleMailKey(other, "3")).toBeNull();
  });
  it("⌘K 등 다른 조합은 null 로 App 전역에 넘긴다", () => {
    expect(handleMailKey(meta, "k")).toBeNull();
    expect(handleMailKey(meta, "j")).toBeNull();
  });
  it("/는 검색", () =>
    expect(handleMailKey(other, "/")).toEqual({ type: "OpenSearch" }));
  it("텍스트 포커스면 무시", () =>
    expect(handleMailKey({ focus: "text", detailOpen: false, meta: false }, "e")).toBeNull());
  it("텍스트 포커스에서 t는 무시", () =>
    expect(handleMailKey({ focus: "text", detailOpen: false, meta: false }, "t")).toBeNull());
  it("Escape는 상세 열림 시 닫기", () =>
    expect(handleMailKey({ focus: "other", detailOpen: true, meta: false }, "Escape")).toEqual({
      type: "Close",
    }));
  it("알 수 없는 키는 null", () =>
    expect(handleMailKey(other, "z")).toBeNull());
});
