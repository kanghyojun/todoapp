import { describe, expect, it } from "vitest";
import { arrangeForView, sectionStatusAt, serverStatusFor } from "./now-view";

describe("serverStatusFor", () => {
  it("now 는 status 무지정으로 보낸다", () => {
    expect(serverStatusFor("now")).toBeUndefined();
    expect(serverStatusFor("done")).toBe("done");
    expect(serverStatusFor(undefined)).toBeUndefined();
  });
});

describe("arrangeForView", () => {
  it("now 뷰는 done·deferred 를 빼고 in_progress 를 todo 앞으로 옮긴다", () => {
    const todos = [
      { id: "a", status: "todo" },
      { id: "b", status: "done" },
      { id: "c", status: "in_progress" },
      { id: "d", status: "deferred" },
      { id: "e", status: "todo" },
    ] as const;
    expect(arrangeForView("now", todos)).toEqual([
      { id: "c", status: "in_progress" },
      { id: "a", status: "todo" },
      { id: "e", status: "todo" },
    ]);
  });

  // 검색(bm25) 결과처럼 같은 상태끼리 섞여 들어와도 입력 순서를 지켜야
  // 관련도 순이 흐트러지지 않는다.
  it("같은 상태끼리는 입력 순서를 유지한다", () => {
    const todos = [
      { id: "a", status: "todo" },
      { id: "b", status: "todo" },
      { id: "c", status: "in_progress" },
    ] as const;
    expect(arrangeForView("now", todos).map((todo) => todo.id)).toEqual(["c", "a", "b"]);
  });

  it("now 가 아닌 뷰는 입력을 그대로 돌려준다", () => {
    const todos = [
      { id: "a", status: "done" },
      { id: "b", status: "todo" },
    ] as const;
    expect(arrangeForView(undefined, todos)).toEqual(todos);
    expect(arrangeForView("done", todos)).toEqual(todos);
  });
});

describe("sectionStatusAt", () => {
  it("now 뷰에서 index 0 과 상태 경계에만 머리를 붙인다", () => {
    const todos = [
      { id: "a", status: "in_progress" },
      { id: "b", status: "in_progress" },
      { id: "c", status: "todo" },
    ] as const;
    expect(sectionStatusAt("now", todos, 0)).toBe("in_progress");
    expect(sectionStatusAt("now", todos, 1)).toBeNull();
    expect(sectionStatusAt("now", todos, 2)).toBe("todo");
  });

  it("진행 중이 없으면 index 0 머리가 todo 다", () => {
    const todos = [{ id: "a", status: "todo" }] as const;
    expect(sectionStatusAt("now", todos, 0)).toBe("todo");
  });

  it("now 가 아닌 뷰는 항상 null", () => {
    const todos = [{ id: "a", status: "todo" }] as const;
    expect(sectionStatusAt(undefined, todos, 0)).toBeNull();
    expect(sectionStatusAt("done", todos, 0)).toBeNull();
  });

  it("빈 목록과 범위 밖 index 는 null", () => {
    expect(sectionStatusAt("now", [], 0)).toBeNull();
    const todos = [{ id: "a", status: "todo" }] as const;
    expect(sectionStatusAt("now", todos, 5)).toBeNull();
  });
});
