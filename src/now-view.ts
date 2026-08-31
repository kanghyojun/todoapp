import type { Status } from "./domain";

// 필터 칩이 가리키는 뷰. Status 넷에 미완료 모아보기("now")를 더한 것.
// undefined 는 전체다. 서버는 "now" 를 모르므로 보내기 전에 벗겨야 한다.
export type FilterView = Status | "now";

// "지금" 뷰가 본 목록에 보여주는 상태와 섹션 순서. 진행 중이 먼저다.
export const NOW_SECTIONS: readonly Status[] = ["in_progress", "todo"];

/** 서버 Filter 의 status 로 옮길 값. "now" 는 status 무지정(=미보류 전체)으로 보낸다. */
export function serverStatusFor(view: FilterView | undefined): Status | undefined {
  return view === "now" ? undefined : view;
}

/**
 * 뷰에 맞춰 목록을 다듬는다. "now" 면 done·deferred 를 빼고
 * NOW_SECTIONS 순서로 안정 재배열한다(섹션 안 순서는 서버가 준 그대로).
 * 다른 뷰는 서버가 이미 걸러 줬으니 손대지 않는다.
 */
export function arrangeForView<T extends { status: Status }>(
  view: FilterView | undefined,
  todos: readonly T[],
): readonly T[] {
  if (view !== "now") return todos;
  return NOW_SECTIONS.flatMap((status) => todos.filter((todo) => todo.status === status));
}

/**
 * index 앞에 섹션 머리를 끼워야 하면 그 상태를, 아니면 null 을 돌려준다.
 * arrangeForView 가 상태를 연속으로 묶어 놨다는 전제 위에서만 옳다.
 */
export function sectionStatusAt<T extends { status: Status }>(
  view: FilterView | undefined,
  todos: readonly T[],
  index: number,
): Status | null {
  if (view !== "now") return null;
  const current = todos[index];
  if (current === undefined) return null;
  if (index === 0) return current.status;
  const previous = todos[index - 1];
  if (previous === undefined) return current.status;
  return previous.status !== current.status ? current.status : null;
}
