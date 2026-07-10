import { describe, expect, it } from "vitest";
import {
  THEME_STORAGE_KEY,
  applyPreference,
  cyclePreference,
  readPreference,
  savePreference,
  type ThemeStorage,
  type ThemeTarget,
} from "./theme";

function memoryStorage(initial: Record<string, string> = {}): ThemeStorage {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

// Safari 프라이빗 모드처럼 저장소가 통째로 던지는 기기가 있다.
// 키체인과 같은 규칙이다. 읽지 못하는 것은 없는 것과 같다.
function hostileStorage(): ThemeStorage {
  return {
    getItem: () => {
      throw new Error("localStorage is not available");
    },
    setItem: () => {
      throw new Error("localStorage is not available");
    },
  };
}

function memoryTarget(
  initial: Record<string, string> = {},
): ThemeTarget & { attributes: Map<string, string> } {
  const attributes = new Map(Object.entries(initial));
  return {
    attributes,
    setAttribute: (name, value) => void attributes.set(name, value),
    removeAttribute: (name) => void attributes.delete(name),
  };
}

describe("cyclePreference", () => {
  it("walks auto to light to dark and back to auto", () => {
    expect(cyclePreference("auto")).toBe("light");
    expect(cyclePreference("light")).toBe("dark");
    expect(cyclePreference("dark")).toBe("auto");
  });
});

describe("readPreference", () => {
  it("falls back to auto when nothing is stored", () => {
    expect(readPreference(memoryStorage())).toBe("auto");
  });

  it("returns the stored preference", () => {
    expect(readPreference(memoryStorage({ [THEME_STORAGE_KEY]: "dark" }))).toBe("dark");
    expect(readPreference(memoryStorage({ [THEME_STORAGE_KEY]: "light" }))).toBe("light");
  });

  it("falls back to auto when the stored value is not a preference", () => {
    expect(readPreference(memoryStorage({ [THEME_STORAGE_KEY]: "solarized" }))).toBe("auto");
    expect(readPreference(memoryStorage({ [THEME_STORAGE_KEY]: "" }))).toBe("auto");
  });

  it("falls back to auto when the storage throws", () => {
    expect(readPreference(hostileStorage())).toBe("auto");
  });
});

describe("savePreference", () => {
  it("writes the preference under the theme key", () => {
    const storage = memoryStorage();
    savePreference(storage, "dark");
    expect(storage.getItem(THEME_STORAGE_KEY)).toBe("dark");
  });

  it("stays quiet when the storage throws", () => {
    expect(() => savePreference(hostileStorage(), "dark")).not.toThrow();
  });
});

describe("applyPreference", () => {
  it("stamps data-theme for an explicit preference", () => {
    const target = memoryTarget();
    applyPreference(target, "dark");
    expect(target.attributes.get("data-theme")).toBe("dark");

    applyPreference(target, "light");
    expect(target.attributes.get("data-theme")).toBe("light");
  });

  it("removes data-theme for auto so the OS setting wins", () => {
    const target = memoryTarget({ "data-theme": "dark" });
    applyPreference(target, "auto");
    expect(target.attributes.has("data-theme")).toBe(false);
  });
});
