import { describe, expect, it } from "vitest";
import { prepareMailHtml } from "./mail-html";

describe("prepareMailHtml", () => {
  it("doctype 가 있으면 그 뒤에 머리말을 넣는다", () => {
    const out = prepareMailHtml("<!DOCTYPE html><html><body>본문</body></html>");
    // doctype 앞에 뭐가 붙으면 quirks mode 로 떨어져 레이아웃이 틀어진다.
    expect(out.startsWith("<!DOCTYPE html>")).toBe(true);
  });

  it("doctype 이 없으면 맨 앞에 붙인다", () => {
    const out = prepareMailHtml("<p>본문</p>");
    expect(out.startsWith("<meta")).toBe(true);
    expect(out.endsWith("<p>본문</p>")).toBe(true);
  });

  it("doctype 앞 공백과 대소문자를 가리지 않는다", () => {
    const out = prepareMailHtml("\n  <!doctype html>\n<p>본문</p>");
    expect(out.startsWith("\n  <!doctype html>")).toBe(true);
  });

  it("링크를 최상위로 올리는 base 를 넣는다", () => {
    expect(prepareMailHtml("<p>x</p>")).toContain('<base target="_top">');
  });

  it("중첩 프레임을 막는다", () => {
    // 이게 빠지면 메일을 여는 것만으로 중첩 iframe 주소가 브라우저에 열린다.
    const out = prepareMailHtml("<p>x</p>");
    expect(out).toContain("frame-src 'none'");
    expect(out).toContain("object-src 'none'");
  });

  it("default-src 는 걸지 않는다", () => {
    // 걸면 이미지와 스타일까지 막혀 본문이 깨진다.
    expect(prepareMailHtml("<p>x</p>")).not.toContain("default-src");
  });

  it("메일에 지정이 없을 때 쓸 기본 폰트를 넣는다", () => {
    // 없으면 브라우저 기본 serif 가 잡혀 한글이 명조/궁서로 나온다.
    const out = prepareMailHtml("<p>x</p>");
    expect(out).toContain("system-ui");
    // html 에만 걸어야 메일이 가진 font-family 가 이긴다.
    expect(out).toContain("<style>html{");
  });

  it("원문 HTML 을 그대로 보존한다", () => {
    const html = "<!DOCTYPE html><html><body><p>본문</p></body></html>";
    const out = prepareMailHtml(html);
    const stripped = out.replace(
      /<meta[^>]*>|<base[^>]*>|<style>.*?<\/style>/g,
      "",
    );
    expect(stripped).toBe(html);
  });
});
