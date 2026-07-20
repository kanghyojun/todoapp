import { describe, expect, it } from "vitest";
import { linkifyText } from "./linkify";

const links = (text: string): string[] =>
  linkifyText(text)
    .filter((segment) => segment.kind === "link")
    .map((segment) => segment.value);

// 조각을 다시 이어붙이면 원문이 나와야 한다. 어긋나면 본문에서 글자가
// 사라지거나 중복된다.
const rejoined = (text: string): string =>
  linkifyText(text)
    .map((segment) => segment.value)
    .join("");

describe("linkifyText", () => {
  it("https 링크를 찾아낸다", () =>
    expect(links("보고서는 https://example.com/a 에 있습니다")).toEqual([
      "https://example.com/a",
    ]));

  it("http 링크도 찾아낸다", () =>
    expect(links("http://example.com")).toEqual(["http://example.com"]));

  it("한 줄에 여러 링크를 모두 찾아낸다", () =>
    expect(links("https://a.com 과 https://b.com")).toEqual([
      "https://a.com",
      "https://b.com",
    ]));

  it("문장 끝 마침표는 링크에서 뺀다", () =>
    expect(links("https://example.com/a. 끝")).toEqual(["https://example.com/a"]));

  it("짝이 맞는 괄호는 링크에 남긴다", () =>
    expect(links("https://ko.wikipedia.org/wiki/Foo_(bar)")).toEqual([
      "https://ko.wikipedia.org/wiki/Foo_(bar)",
    ]));

  it("짝이 안 맞는 닫는 괄호는 링크에서 뺀다", () =>
    expect(links("(https://example.com/a)")).toEqual(["https://example.com/a"]));

  it("스킴만 있으면 링크로 보지 않는다", () =>
    expect(links("https:// 로 시작하는 주소")).toEqual([]));

  it("링크가 없으면 텍스트 한 조각", () =>
    expect(linkifyText("링크 없는 본문")).toEqual([
      { kind: "text", value: "링크 없는 본문" },
    ]));

  it("빈 문자열은 조각이 없다", () => expect(linkifyText("")).toEqual([]));

  it("조각을 이어붙이면 원문이 그대로 나온다", () => {
    const text = "앞 https://a.com/x. 가운데 (https://b.com/y) 뒤";
    expect(rejoined(text)).toBe(text);
  });

  it("링크로 시작하고 링크로 끝나도 원문이 보존된다", () => {
    const text = "https://a.com 사이 https://b.com";
    expect(rejoined(text)).toBe(text);
  });
});
