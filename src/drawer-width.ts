export type DrawerKind = "todo" | "mail";

export const DRAWER_WIDTH_KEYS: Readonly<Record<DrawerKind, string>> = {
  todo: "todo.detailWidth",
  mail: "todo.mailDetailWidth",
};

// 메일 본문은 넓게 봐야 읽힌다. 그래서 기본값이 다르다.
export const DEFAULT_DRAWER_WIDTH: Readonly<Record<DrawerKind, number>> = {
  todo: 420,
  mail: 800,
};

// 폭을 CSS 변수로 두면 .detail-panel.shifted 의 right 가 메일 드로어 폭을
// 그대로 따라간다. 두 값을 따로 관리하면 어긋나서 드로어가 겹친다.
export const DRAWER_WIDTH_VARS: Readonly<Record<DrawerKind, string>> = {
  todo: "--todo-detail-width",
  mail: "--mail-detail-width",
};

export const MIN_DRAWER_WIDTH = 280;

// 목록이 아예 안 보이게 끌어 놓으면 되돌릴 방법이 사라진다.
const MIN_VISIBLE_LIST = 160;

/** `localStorage` 가 구조적으로 만족한다. */
export interface DrawerStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function clampDrawerWidth(width: number, viewportWidth: number): number {
  const max = Math.max(MIN_DRAWER_WIDTH, viewportWidth - MIN_VISIBLE_LIST);
  return Math.round(Math.min(Math.max(width, MIN_DRAWER_WIDTH), max));
}

// 드로어는 오른쪽에 붙어 있다. 왼쪽 가장자리를 끌면 폭은 화면 오른쪽 끝까지의
// 거리다.
export function widthFromPointer(
  clientX: number,
  viewportWidth: number,
): number {
  return clampDrawerWidth(viewportWidth - clientX, viewportWidth);
}

// 저장소를 읽지 못하는 것은 저장된 값이 없는 것과 같다. theme 과 같은 규칙이다.
export function readDrawerWidth(
  storage: DrawerStorage,
  kind: DrawerKind,
  viewportWidth: number,
): number {
  let stored: string | null;
  try {
    stored = storage.getItem(DRAWER_WIDTH_KEYS[kind]);
  } catch {
    return DEFAULT_DRAWER_WIDTH[kind];
  }
  if (stored === null) return DEFAULT_DRAWER_WIDTH[kind];
  const parsed = Number.parseInt(stored, 10);
  if (Number.isNaN(parsed)) return DEFAULT_DRAWER_WIDTH[kind];
  return clampDrawerWidth(parsed, viewportWidth);
}

export function saveDrawerWidth(
  storage: DrawerStorage,
  kind: DrawerKind,
  width: number,
): void {
  try {
    storage.setItem(DRAWER_WIDTH_KEYS[kind], String(Math.round(width)));
  } catch {
    // 표시 설정일 뿐이다. 저장하지 못했다고 앱이 멈출 이유가 없다.
  }
}
