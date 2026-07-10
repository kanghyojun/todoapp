import { describe, expect, it, vi } from "vitest";
import { HttpClient, NotFoundError } from "./client";

const TODO = {
  id: "0197f000-0000-7000-8000-000000000001",
  title: "Ship M3",
  description: "",
  status: "todo",
  priority: "high",
  due_date: null,
  completed_at: null,
  created_at: "2026-07-10T00:00:00Z",
  updated_at: "2026-07-10T00:00:00Z",
  deleted_at: null,
};

describe("HttpClient", () => {
  // 브라우저의 fetch 는 this 가 window 가 아니면 Illegal invocation 을 던진다.
  // 기본 fetcher 를 필드에 그냥 담으면 this.fetcher(...) 가 클라이언트에 묶인다.
  it("does not bind the default fetcher to the client instance", async () => {
    const receivers: unknown[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = function (this: unknown): Promise<Response> {
      receivers.push(this);
      return Promise.resolve(
        new Response("[]", {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
      );
    } as typeof globalThis.fetch;

    try {
      const client = new HttpClient("token", "http://example.test/api/v1");
      await client.list({});
    } finally {
      globalThis.fetch = original;
    }

    expect(receivers).toHaveLength(1);
    expect(receivers[0]).not.toBeInstanceOf(HttpClient);
  });

  it("maps 404 to NotFoundError and preserves the server message", async () => {
    const fetcher = vi.fn(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({ error: { code: "not_found", message: "todo not found" } }),
          { status: 404, headers: { "Content-Type": "application/json" } },
        ),
      ),
    );
    const client = new HttpClient("token", "http://example.test/api/v1", fetcher);
    await expect(client.get("missing")).rejects.toMatchObject({
      name: NotFoundError.name,
      message: "todo not found",
    });
  });

  it("sends priority as its wire string and never as a number", async () => {
    const requests: RequestInit[] = [];
    const fetcher = async (
      _input: RequestInfo | URL,
      init?: RequestInit,
    ): Promise<Response> => {
      if (init !== undefined) requests.push(init);
      return new Response(JSON.stringify(TODO), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    };
    const client = new HttpClient("token", "http://example.test/api/v1", fetcher);
    await client.update(TODO.id, { priority: "high" });
    const init = requests[0];
    expect(typeof init?.body).toBe("string");
    const body: unknown = JSON.parse(String(init?.body));
    expect(body).toEqual({ priority: "high" });
    expect(JSON.stringify(body)).not.toContain('"priority":2');
  });
});
