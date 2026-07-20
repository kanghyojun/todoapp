export const COMPLETED_VISIBLE_DAYS = 7;

export const SHOW_OLD_COMPLETED_KEY = "todo.showOldCompleted";

/** `localStorage` 가 구조적으로 만족한다. */
export interface PreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

// 저장소를 읽지 못하는 것은 저장된 값이 없는 것과 같다. theme 과 같은 규칙이다.
// 기본값은 숨김이다. 오래 전에 끝낸 일이 목록을 채우는 게 원래 문제였다.
export function readShowOldCompleted(storage: PreferenceStorage): boolean {
  try {
    return storage.getItem(SHOW_OLD_COMPLETED_KEY) === "true";
  } catch {
    return false;
  }
}

export function saveShowOldCompleted(
  storage: PreferenceStorage,
  next: boolean,
): void {
  try {
    storage.setItem(SHOW_OLD_COMPLETED_KEY, next ? "true" : "false");
  } catch {
    // 표시 설정일 뿐이다. 저장하지 못했다고 앱이 멈출 이유가 없다.
  }
}

// 로컬 달력 기준으로 찍는다. toISOString 을 쓰면 UTC 로 넘어가 한국에서는
// 하루가 밀린다.
function formatLocalDate(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

// 서버에 넘길 completed_since 값. 오래된 것도 보기로 했으면 제한을 걸지 않는다.
export function completedSinceFor(
  showOld: boolean,
  today: Date,
): string | undefined {
  if (showOld) return undefined;
  const cutoff = new Date(today);
  cutoff.setDate(cutoff.getDate() - COMPLETED_VISIBLE_DAYS);
  return formatLocalDate(cutoff);
}
