import { describe, expect, it } from "vitest";
import { handleMailKey } from "./keyboard-mail";

const other = { focus: "other" as const, detailOpen: false };

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
  it("1/2/3은 폴더 전환", () => {
    expect(handleMailKey(other, "1")).toEqual({ type: "SetFolder", folder: "inbox" });
    expect(handleMailKey(other, "2")).toEqual({ type: "SetFolder", folder: "archive" });
    expect(handleMailKey(other, "3")).toEqual({ type: "SetFolder", folder: "all" });
  });
  it("/는 검색", () =>
    expect(handleMailKey(other, "/")).toEqual({ type: "OpenSearch" }));
  it("텍스트 포커스면 무시", () =>
    expect(handleMailKey({ focus: "text", detailOpen: false }, "e")).toBeNull());
  it("Escape는 상세 열림 시 닫기", () =>
    expect(handleMailKey({ focus: "other", detailOpen: true }, "Escape")).toEqual({
      type: "Close",
    }));
  it("알 수 없는 키는 null", () =>
    expect(handleMailKey(other, "z")).toBeNull());
});
