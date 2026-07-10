export type ThemePreference = "auto" | "light" | "dark";

export const THEME_STORAGE_KEY = "todo.theme";

/** `localStorage` 가 구조적으로 만족한다. */
export interface ThemeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** `document.documentElement` 가 구조적으로 만족한다. */
export interface ThemeTarget {
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

const CYCLE: Readonly<Record<ThemePreference, ThemePreference>> = {
  auto: "light",
  light: "dark",
  dark: "auto",
};

function isPreference(value: string | null): value is ThemePreference {
  return value === "auto" || value === "light" || value === "dark";
}

export function cyclePreference(current: ThemePreference): ThemePreference {
  return CYCLE[current];
}

// 저장소를 읽지 못하는 것은 저장된 값이 없는 것과 같다. 키체인과 같은 규칙이다.
export function readPreference(storage: ThemeStorage): ThemePreference {
  let stored: string | null;
  try {
    stored = storage.getItem(THEME_STORAGE_KEY);
  } catch {
    return "auto";
  }
  return isPreference(stored) ? stored : "auto";
}

export function savePreference(storage: ThemeStorage, next: ThemePreference): void {
  try {
    storage.setItem(THEME_STORAGE_KEY, next);
  } catch {
    // 표시 설정일 뿐이다. 저장하지 못했다고 앱이 멈출 이유가 없다.
  }
}

// auto 는 속성을 지운다. 그래야 color-scheme 이 OS 설정으로 돌아간다.
export function applyPreference(target: ThemeTarget, preference: ThemePreference): void {
  if (preference === "auto") {
    target.removeAttribute("data-theme");
    return;
  }
  target.setAttribute("data-theme", preference);
}
