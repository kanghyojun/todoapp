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
  deferred_until: null,
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

  const respondWith = (payload: unknown) =>
    async (): Promise<Response> =>
      new Response(JSON.stringify(payload), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });

  it("decodes the linear link when the server sends one", async () => {
    const client = new HttpClient(
      "token",
      "http://example.test/api/v1",
      respondWith({
        ...TODO,
        linear: { identifier: "PI-42", url: "https://linear.app/x/issue/PI-42" },
      }),
    );
    const todo = await client.get(TODO.id);
    expect(todo.linear).toEqual({
      identifier: "PI-42",
      url: "https://linear.app/x/issue/PI-42",
    });
  });

  it("treats a missing or null linear link as null", async () => {
    const withNull = new HttpClient(
      "token",
      "http://example.test/api/v1",
      respondWith({ ...TODO, linear: null }),
    );
    expect((await withNull.get(TODO.id)).linear).toBeNull();

    const withoutField = new HttpClient(
      "token",
      "http://example.test/api/v1",
      respondWith(TODO),
    );
    expect((await withoutField.get(TODO.id)).linear).toBeNull();
  });

  it("decodes linear status from snake_case", async () => {
    const client = new HttpClient(
      "token",
      "http://example.test/api/v1",
      respondWith({
        configured: true,
        key_store_available: true,
        pending: 0,
        failing: 2,
        needs_choice: [],
      }),
    );
    expect(await client.linearStatus()).toEqual({
      configured: true,
      keyStoreAvailable: true,
      failing: 2,
    });
  });

  it("posts the api key under the wire field name", async () => {
    const requests: RequestInit[] = [];
    const fetcher = async (
      _input: RequestInfo | URL,
      init?: RequestInit,
    ): Promise<Response> => {
      if (init !== undefined) requests.push(init);
      return new Response(null, { status: 204 });
    };
    const client = new HttpClient("token", "http://example.test/api/v1", fetcher);
    await client.setLinearKey("lin_api_secret");
    const body: unknown = JSON.parse(String(requests[0]?.body));
    expect(body).toEqual({ api_key: "lin_api_secret" });
  });

  it("defers with the natural-language date under 'until'", async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const fetcher = async (
      input: RequestInfo | URL,
      init?: RequestInit,
    ): Promise<Response> => {
      requests.push({ url: String(input), init });
      return new Response(JSON.stringify({ ...TODO, status: "deferred" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    };
    const client = new HttpClient("token", "http://example.test/api/v1", fetcher);
    const todo = await client.defer(TODO.id, "next week");
    expect(todo.status).toBe("deferred");
    expect(requests[0]?.url).toContain(`/todos/${TODO.id}/defer`);
    expect(JSON.parse(String(requests[0]?.init?.body))).toEqual({ until: "next week" });
  });
});
