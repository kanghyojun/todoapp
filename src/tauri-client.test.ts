import { describe, expect, it, vi } from "vitest";
import { NotFoundError, TauriClient, type InvokeFn, type ListenFn } from "./client";

const TODO = {
  id: "0197f000-0000-7000-8000-000000000001",
  title: "Ship M4",
  description: "",
  status: "todo",
  priority: "high",
  due_date: "2026-07-11",
  completed_at: null,
  created_at: "2026-07-10T00:00:00Z",
  updated_at: "2026-07-10T00:00:00Z",
  deleted_at: null,
  deferred_until: null,
  linear: null,
  email: null,
};

describe("TauriClient", () => {
  it("uses IPC commands and decodes the same todo wire shape", async () => {
    const invoke = vi.fn<InvokeFn>(async (command) =>
      command === "list" ? [TODO] : TODO,
    );
    const listen = vi.fn<ListenFn>();
    const client = new TauriClient(invoke, listen);

    await expect(client.list({ priority: "high" })).resolves.toEqual([TODO]);
    await expect(
      client.update(TODO.id, { due_date: "tomorrow", priority: "urgent" }),
    ).resolves.toEqual(TODO);
    expect(invoke).toHaveBeenNthCalledWith(1, "list", {
      filter: { priority: "high" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "update", {
      id: TODO.id,
      patch: { due_date: "tomorrow", priority: "urgent" },
    });
  });

  it("maps command errors and server status", async () => {
    const invoke: InvokeFn = async (command) => {
      if (command === "get") {
        throw { code: "not_found", message: "todo not found" };
      }
      return { running: false, error: "2470 포트가 사용 중입니다" };
    };
    const client = new TauriClient(invoke, vi.fn<ListenFn>());

    await expect(client.get("missing")).rejects.toBeInstanceOf(NotFoundError);
    await expect(client.serverStatus()).resolves.toEqual({
      running: false,
      error: "2470 포트가 사용 중입니다",
    });
  });

  it("creates a todo from email through IPC and decodes its email link", async () => {
    const input = {
      title: "Reply to Kim",
      account_id: "account-1",
      gmail_id: "message-1",
      thread_id: "thread-1",
      subject: "Hello",
      from_name: "Kim",
      from_email: "kim@example.com",
    };
    const linkedTodo = {
      ...TODO,
      title: input.title,
      email: {
        account_id: input.account_id,
        gmail_id: input.gmail_id,
        thread_id: input.thread_id,
        subject: input.subject,
        from_name: input.from_name,
        from_email: input.from_email,
      },
    };
    const invoke = vi.fn<InvokeFn>(async () => linkedTodo);
    const client = new TauriClient(invoke, vi.fn<ListenFn>());

    await expect(client.createFromEmail(input)).resolves.toMatchObject({
      title: "Reply to Kim",
      email: {
        account_id: "account-1",
        gmail_id: "message-1",
        subject: "Hello",
      },
    });
    expect(invoke).toHaveBeenCalledWith("create_todo_from_email", { input });
  });

  it("maps the Linear pull response exactly like HttpClient", async () => {
    const invoke: InvokeFn = async () => ({
      created: 2,
      skipped: 3,
      closed_locally: 1,
    });
    const client = new TauriClient(invoke, vi.fn<ListenFn>());

    await expect(client.pullLinear()).resolves.toEqual({
      created: 2,
      skipped: 3,
      completed: undefined,
    });
  });

  it("listens for todo:changed without polling and cleans up", async () => {
    const stop = vi.fn();
    let handler: ((event: unknown) => void) | undefined;
    const listen: ListenFn = vi.fn(async (_event, nextHandler) => {
      handler = nextHandler;
      return stop;
    });
    const onChange = vi.fn();
    const client = new TauriClient(vi.fn<InvokeFn>(), listen);

    const unsubscribe = client.subscribe(onChange);
    await Promise.resolve();
    handler?.({});
    expect(listen).toHaveBeenCalledWith("todo:changed", onChange);
    expect(onChange).toHaveBeenCalledOnce();

    unsubscribe();
    expect(stop).toHaveBeenCalledOnce();
  });
});
