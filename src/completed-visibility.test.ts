import { describe, expect, it } from "vitest";
import {
  SHOW_OLD_COMPLETED_KEY,
  completedSinceFor,
  readShowOldCompleted,
  saveShowOldCompleted,
} from "./completed-visibility";

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  };
}

const broken = {
  getItem(): string {
    throw new Error("storage is unavailable");
  },
  setItem(): void {
    throw new Error("storage is unavailable");
  },
};

describe("readShowOldCompleted", () => {
  it("저장된 값이 없으면 숨긴다", () =>
    expect(readShowOldCompleted(memoryStorage())).toBe(false));

  it("true 로 저장돼 있으면 보여준다", () =>
    expect(
      readShowOldCompleted(memoryStorage({ [SHOW_OLD_COMPLETED_KEY]: "true" })),
    ).toBe(true));

  it("저장소를 못 읽으면 기본값으로 간다", () =>
    expect(readShowOldCompleted(broken)).toBe(false));
});

describe("saveShowOldCompleted", () => {
  it("값을 저장한다", () => {
    const storage = memoryStorage();
    saveShowOldCompleted(storage, true);
    expect(storage.values.get(SHOW_OLD_COMPLETED_KEY)).toBe("true");
    saveShowOldCompleted(storage, false);
    expect(storage.values.get(SHOW_OLD_COMPLETED_KEY)).toBe("false");
  });

  it("저장소가 죽어 있어도 던지지 않는다", () =>
    expect(() => saveShowOldCompleted(broken, true)).not.toThrow());
});

describe("completedSinceFor", () => {
  it("오래된 것도 보기면 제한을 걸지 않는다", () =>
    expect(completedSinceFor(true, new Date(2026, 6, 20))).toBeUndefined());

  it("숨김이면 7일 전 날짜를 준다", () =>
    expect(completedSinceFor(false, new Date(2026, 6, 20))).toBe("2026-07-13"));

  it("월을 넘어가도 맞게 센다", () =>
    expect(completedSinceFor(false, new Date(2026, 6, 3))).toBe("2026-06-26"));

  it("해를 넘어가도 맞게 센다", () =>
    expect(completedSinceFor(false, new Date(2026, 0, 3))).toBe("2025-12-27"));

  // toISOString 을 쓰면 UTC 로 넘어가 한국 시간대에서 하루가 밀린다.
  it("자정 직후 로컬 시각에도 날짜가 밀리지 않는다", () =>
    expect(completedSinceFor(false, new Date(2026, 6, 20, 0, 30))).toBe(
      "2026-07-13",
    ));
});
