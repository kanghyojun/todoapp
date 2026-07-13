import { describe, expect, it } from "vitest";
import { decodeAccount, decodeBody, decodeMailListItem } from "./client";

describe("decodeMailListItem", () => {
  const valid = {
    account_id: "a",
    account_email: "a@x.com",
    account_color: "#268bd2",
    gmail_id: "m1",
    thread_id: "t1",
    from_name: "Kim",
    from_email: "kim@x.com",
    subject: "hi",
    snippet: "...",
    internal_date: 123,
    in_inbox: true,
    is_unread: false,
    has_todo: false,
  };

  it("정상 페이로드를 파싱한다", () => {
    const item = decodeMailListItem(valid);
    expect(item.gmail_id).toBe("m1");
    expect(item.in_inbox).toBe(true);
    expect(item.internal_date).toBe(123);
    expect(item.has_todo).toBe(false);
  });

  it("필드 누락 시 던진다", () => {
    expect(() => decodeMailListItem({ gmail_id: "m1" })).toThrow();
  });

  it("타입이 어긋나면 던진다", () => {
    expect(() =>
      decodeMailListItem({ ...valid, internal_date: "123" }),
    ).toThrow();
  });
});

describe("decodeAccount", () => {
  it("정상 계정을 파싱한다", () => {
    const account = decodeAccount({
      id: "a",
      email: "a@x.com",
      color: "#268bd2",
      sync_state: "idle",
      last_error: null,
    });
    expect(account.email).toBe("a@x.com");
    expect(account.last_error).toBeNull();
  });

  it("잘못된 계정은 던진다", () => {
    expect(() => decodeAccount({ id: "a" })).toThrow();
  });
});

describe("decodeBody", () => {
  it("본문을 파싱한다", () => {
    const body = decodeBody({
      gmail_id: "m1",
      body_text: "hello",
      body_html: null,
    });
    expect(body.body_text).toBe("hello");
    expect(body.body_html).toBeNull();
  });
});
