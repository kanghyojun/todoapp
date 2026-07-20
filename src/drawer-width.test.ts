import { describe, expect, it } from "vitest";
import {
  DEFAULT_DRAWER_WIDTH,
  DRAWER_WIDTH_KEYS,
  MIN_DRAWER_WIDTH,
  clampDrawerWidth,
  readDrawerWidth,
  saveDrawerWidth,
  widthFromPointer,
} from "./drawer-width";

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

describe("clampDrawerWidth", () => {
  it("최소 폭 아래로는 안 줄어든다", () =>
    expect(clampDrawerWidth(50, 1400)).toBe(MIN_DRAWER_WIDTH));

  it("목록이 보일 만큼은 남긴다", () =>
    // 다 덮어버리면 드로어를 되돌릴 방법이 사라진다.
    expect(clampDrawerWidth(9999, 1400)).toBe(1240));

  it("범위 안이면 그대로 둔다", () =>
    expect(clampDrawerWidth(600, 1400)).toBe(600));

  it("창이 최소 폭보다 좁아도 최소 폭을 지킨다", () =>
    expect(clampDrawerWidth(500, 200)).toBe(MIN_DRAWER_WIDTH));
});

describe("widthFromPointer", () => {
  it("왼쪽 가장자리를 끈 만큼이 폭이 된다", () =>
    expect(widthFromPointer(800, 1400)).toBe(600));

  it("왼쪽 끝까지 끌어도 목록 자리는 남는다", () =>
    expect(widthFromPointer(0, 1400)).toBe(1240));

  it("오른쪽 끝까지 끌어도 최소 폭을 지킨다", () =>
    expect(widthFromPointer(1400, 1400)).toBe(MIN_DRAWER_WIDTH));
});

describe("readDrawerWidth", () => {
  it("저장된 값이 없으면 기본값", () => {
    expect(readDrawerWidth(memoryStorage(), "todo", 1400)).toBe(
      DEFAULT_DRAWER_WIDTH.todo,
    );
    expect(readDrawerWidth(memoryStorage(), "mail", 1400)).toBe(
      DEFAULT_DRAWER_WIDTH.mail,
    );
  });

  it("저장된 값을 읽는다", () =>
    expect(
      readDrawerWidth(
        memoryStorage({ [DRAWER_WIDTH_KEYS.mail]: "640" }),
        "mail",
        1400,
      ),
    ).toBe(640));

  // 넓은 화면에서 저장해 두고 좁은 화면에서 열면 드로어가 화면을 다 덮는다.
  it("지금 창 크기에 맞게 잘라 준다", () =>
    expect(
      readDrawerWidth(
        memoryStorage({ [DRAWER_WIDTH_KEYS.mail]: "3000" }),
        "mail",
        1000,
      ),
    ).toBe(840));

  it("깨진 값이면 기본값", () =>
    expect(
      readDrawerWidth(
        memoryStorage({ [DRAWER_WIDTH_KEYS.todo]: "넓게" }),
        "todo",
        1400,
      ),
    ).toBe(DEFAULT_DRAWER_WIDTH.todo));

  it("저장소를 못 읽으면 기본값", () =>
    expect(readDrawerWidth(broken, "todo", 1400)).toBe(
      DEFAULT_DRAWER_WIDTH.todo,
    ));
});

describe("saveDrawerWidth", () => {
  it("정수로 저장한다", () => {
    const storage = memoryStorage();
    saveDrawerWidth(storage, "mail", 640.6);
    expect(storage.values.get(DRAWER_WIDTH_KEYS.mail)).toBe("641");
  });

  it("저장소가 죽어 있어도 던지지 않는다", () =>
    expect(() => saveDrawerWidth(broken, "todo", 500)).not.toThrow());
});
