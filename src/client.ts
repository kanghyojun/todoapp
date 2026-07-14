import type {
  EmailLinkRequest,
  EmailRef,
  Filter,
  LinearRef,
  LinearStatus,
  NewTodo,
  Priority,
  PullResult,
  Status,
  Todo,
  TodoPatch,
} from "./domain";

export interface TodoClient {
  list(filter: Filter): Promise<Todo[]>;
  get(id: string): Promise<Todo>;
  create(input: NewTodo): Promise<Todo>;
  update(id: string, patch: TodoPatch): Promise<Todo>;
  setStatus(id: string, status: Status): Promise<Todo>;
  remove(id: string): Promise<void>;
  restore(id: string): Promise<Todo>;
  defer(id: string, until: string): Promise<Todo>;
  linkLinear(id: string, issueRef: string): Promise<void>;
  createFromEmail(input: EmailLinkRequest): Promise<Todo>;
  pullLinear(): Promise<PullResult>;
  linearStatus(): Promise<LinearStatus>;
  setLinearKey(apiKey: string): Promise<void>;
  openExternal(url: string): Promise<void>;
  subscribe(onChange: () => void): () => void;
}

export interface ServerStatus {
  running: boolean;
  error: string | null;
}

export type InvokeFn = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

export type ListenFn = (
  event: string,
  handler: (event: unknown) => void,
) => Promise<() => void>;

interface ErrorEnvelope {
  error: {
    code: string;
    message: string;
  };
}

type Fetcher = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export class ApiError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly status: number,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export class NotFoundError extends ApiError {
  constructor(message: string, status = 404) {
    super(message, "not_found", status);
    this.name = "NotFoundError";
  }
}

const STATUSES: readonly Status[] = ["todo", "in_progress", "done", "deferred"];
const PRIORITIES: readonly Priority[] = [
  "none",
  "urgent",
  "high",
  "medium",
  "low",
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableString(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

function decodeTodo(value: unknown): Todo {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    typeof value.code !== "string" ||
    typeof value.title !== "string" ||
    typeof value.description !== "string" ||
    !STATUSES.includes(value.status as Status) ||
    !PRIORITIES.includes(value.priority as Priority) ||
    !isNullableString(value.due_date) ||
    !isNullableString(value.completed_at) ||
    typeof value.created_at !== "string" ||
    typeof value.updated_at !== "string" ||
    !isNullableString(value.deleted_at) ||
    !isNullableString(value.deferred_until) ||
    !isValidLinear(value.linear) ||
    !isValidEmail(value.email)
  ) {
    throw new ApiError("server returned an invalid todo", "invalid_response", 0);
  }
  return {
    id: value.id,
    code: value.code,
    title: value.title,
    description: value.description,
    status: value.status as Status,
    priority: value.priority as Priority,
    due_date: value.due_date,
    completed_at: value.completed_at,
    created_at: value.created_at,
    updated_at: value.updated_at,
    deleted_at: value.deleted_at,
    deferred_until: value.deferred_until,
    linear: decodeLinear(value.linear),
    email: decodeEmail(value.email),
  };
}

// linear 는 없거나(undefined) null 이거나 {identifier, url} 이다.
function isValidLinear(value: unknown): boolean {
  if (value === undefined || value === null) {
    return true;
  }
  return (
    isRecord(value) &&
    typeof value.identifier === "string" &&
    typeof value.url === "string"
  );
}

function decodeLinear(value: unknown): LinearRef | null {
  if (isRecord(value) && typeof value.identifier === "string" && typeof value.url === "string") {
    return { identifier: value.identifier, url: value.url };
  }
  return null;
}

function isValidEmail(value: unknown): boolean {
  if (value === undefined || value === null) return true;
  return (
    isRecord(value) &&
    typeof value.account_id === "string" &&
    typeof value.gmail_id === "string"
  );
}

function decodeEmail(value: unknown): EmailRef | null {
  if (
    isRecord(value) &&
    typeof value.account_id === "string" &&
    typeof value.gmail_id === "string"
  ) {
    return {
      account_id: value.account_id,
      gmail_id: value.gmail_id,
      thread_id: typeof value.thread_id === "string" ? value.thread_id : "",
      subject: typeof value.subject === "string" ? value.subject : "",
      from_name: typeof value.from_name === "string" ? value.from_name : "",
      from_email: typeof value.from_email === "string" ? value.from_email : "",
    };
  }
  return null;
}

function decodeTodos(value: unknown): Todo[] {
  if (!Array.isArray(value)) {
    throw new ApiError("server returned an invalid todo list", "invalid_response", 0);
  }
  return value.map(decodeTodo);
}

function isErrorEnvelope(value: unknown): value is ErrorEnvelope {
  return (
    isRecord(value) &&
    isRecord(value.error) &&
    typeof value.error.code === "string" &&
    typeof value.error.message === "string"
  );
}

function decodePullResult(value: unknown): PullResult {
  if (
    !isRecord(value) ||
    typeof value.created !== "number" ||
    typeof value.skipped !== "number" ||
    (value.completed !== undefined && typeof value.completed !== "number")
  ) {
    throw new ApiError(
      "server returned an invalid Linear pull result",
      "invalid_response",
      0,
    );
  }
  return {
    created: value.created,
    skipped: value.skipped,
    completed: value.completed,
  };
}

function decodeServerStatus(value: unknown): ServerStatus {
  if (
    !isRecord(value) ||
    typeof value.running !== "boolean" ||
    !isNullableString(value.error)
  ) {
    throw new ApiError(
      "shell returned an invalid server status",
      "invalid_response",
      0,
    );
  }
  return { running: value.running, error: value.error };
}

function decodeLinearStatus(value: unknown): LinearStatus {
  if (
    !isRecord(value) ||
    typeof value.configured !== "boolean" ||
    typeof value.key_store_available !== "boolean" ||
    typeof value.failing !== "number"
  ) {
    throw new ApiError("server returned an invalid Linear status", "invalid_response", 0);
  }
  return {
    configured: value.configured,
    keyStoreAvailable: value.key_store_available,
    failing: value.failing,
  };
}

function commandError(value: unknown): Error {
  if (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.message === "string"
  ) {
    return value.code === "not_found"
      ? new NotFoundError(value.message, 0)
      : new ApiError(value.message, value.code, 0);
  }
  if (value instanceof Error) return value;
  return new ApiError(
    typeof value === "string" ? value : "IPC command failed",
    "ipc_error",
    0,
  );
}

export class HttpClient implements TodoClient {
  constructor(
    private readonly token: string,
    private readonly baseUrl = "http://127.0.0.1:2470/api/v1",
    // fetch 를 그대로 담으면 this.fetcher(...) 가 이 인스턴스에 묶여
    // 브라우저가 Illegal invocation 을 던진다. 감싸서 전역에 남겨둔다.
    private readonly fetcher: Fetcher = (input, init) => fetch(input, init),
  ) {}

  async list(filter: Filter): Promise<Todo[]> {
    const query = new URLSearchParams();
    for (const [key, value] of Object.entries(filter)) {
      if (value !== undefined && value !== "") {
        query.set(key, String(value));
      }
    }
    const suffix = query.size === 0 ? "" : `?${query.toString()}`;
    return decodeTodos(await this.request(`/todos${suffix}`));
  }

  async get(id: string): Promise<Todo> {
    return decodeTodo(await this.request(`/todos/${encodeURIComponent(id)}`));
  }

  async create(input: NewTodo): Promise<Todo> {
    return decodeTodo(
      await this.request("/todos", {
        method: "POST",
        body: JSON.stringify(input),
      }),
    );
  }

  async update(id: string, patch: TodoPatch): Promise<Todo> {
    return decodeTodo(
      await this.request(`/todos/${encodeURIComponent(id)}`, {
        method: "PATCH",
        body: JSON.stringify(patch),
      }),
    );
  }

  async setStatus(id: string, status: Status): Promise<Todo> {
    return this.update(id, { status });
  }

  async remove(id: string): Promise<void> {
    await this.request(`/todos/${encodeURIComponent(id)}`, { method: "DELETE" });
  }

  async restore(id: string): Promise<Todo> {
    return decodeTodo(
      await this.request(`/todos/${encodeURIComponent(id)}/restore`, {
        method: "POST",
      }),
    );
  }

  async defer(id: string, until: string): Promise<Todo> {
    return decodeTodo(
      await this.request(`/todos/${encodeURIComponent(id)}/defer`, {
        method: "POST",
        body: JSON.stringify({ until }),
      }),
    );
  }

  async linkLinear(id: string, issueRef: string): Promise<void> {
    await this.request(`/todos/${encodeURIComponent(id)}/link/linear`, {
      method: "POST",
      body: JSON.stringify({ issue_ref: issueRef }),
    });
  }

  async createFromEmail(): Promise<Todo> {
    throw new ApiError(
      "이메일에서 할 일 만들기는 데스크톱 앱에서만 지원합니다.",
      "unsupported",
      0,
    );
  }

  async pullLinear(): Promise<PullResult> {
    return decodePullResult(
      await this.request("/linear/pull", { method: "POST" }),
    );
  }

  async linearStatus(): Promise<LinearStatus> {
    return decodeLinearStatus(await this.request("/linear/status"));
  }

  async setLinearKey(apiKey: string): Promise<void> {
    await this.request("/linear/key", {
      method: "POST",
      body: JSON.stringify({ api_key: apiKey }),
    });
  }

  // 브라우저 개발 모드에서는 새 탭으로 연다. Tauri 창은 opener 를 쓴다.
  async openExternal(url: string): Promise<void> {
    window.open(url, "_blank", "noopener,noreferrer");
  }

  subscribe(onChange: () => void): () => void {
    const timer = window.setInterval(onChange, 2_000);
    return () => window.clearInterval(timer);
  }

  private async request(path: string, init: RequestInit = {}): Promise<unknown> {
    const headers = new Headers(init.headers);
    headers.set("Authorization", `Bearer ${this.token}`);
    if (init.body !== undefined) {
      headers.set("Content-Type", "application/json");
    }
    const response = await this.fetcher(`${this.baseUrl}${path}`, {
      ...init,
      headers,
    });
    if (!response.ok) {
      let payload: unknown;
      try {
        payload = await response.json();
      } catch {
        payload = null;
      }
      const code = isErrorEnvelope(payload) ? payload.error.code : "http_error";
      const message = isErrorEnvelope(payload)
        ? payload.error.message
        : `request failed with HTTP ${response.status}`;
      if (response.status === 404 || code === "not_found") {
        throw new NotFoundError(message, response.status);
      }
      throw new ApiError(message, code, response.status);
    }
    if (response.status === 204) {
      return null;
    }
    return response.json() as Promise<unknown>;
  }
}

export class TauriClient implements TodoClient {
  constructor(
    private readonly invoke: InvokeFn,
    private readonly listen: ListenFn,
  ) {}

  async list(filter: Filter): Promise<Todo[]> {
    return decodeTodos(await this.request("list", { filter }));
  }

  async get(id: string): Promise<Todo> {
    return decodeTodo(await this.request("get", { id }));
  }

  async create(input: NewTodo): Promise<Todo> {
    return decodeTodo(await this.request("create", { input }));
  }

  async update(id: string, patch: TodoPatch): Promise<Todo> {
    return decodeTodo(await this.request("update", { id, patch }));
  }

  async setStatus(id: string, status: Status): Promise<Todo> {
    return decodeTodo(await this.request("set_status", { id, status }));
  }

  async remove(id: string): Promise<void> {
    await this.request("delete", { id });
  }

  async restore(id: string): Promise<Todo> {
    return decodeTodo(await this.request("restore", { id }));
  }

  async defer(id: string, until: string): Promise<Todo> {
    return decodeTodo(await this.request("defer", { id, until }));
  }

  async linkLinear(id: string, issueRef: string): Promise<void> {
    await this.request("link_linear", { id, issueRef });
  }

  async createFromEmail(input: EmailLinkRequest): Promise<Todo> {
    return decodeTodo(await this.request("create_todo_from_email", { input }));
  }

  async pullLinear(): Promise<PullResult> {
    return decodePullResult(await this.request("pull_linear"));
  }

  async linearStatus(): Promise<LinearStatus> {
    return decodeLinearStatus(await this.request("linear_status"));
  }

  async setLinearKey(apiKey: string): Promise<void> {
    await this.request("set_linear_key", { apiKey });
  }

  async openExternal(url: string): Promise<void> {
    await this.request("open_external", { url });
  }

  async serverStatus(): Promise<ServerStatus> {
    return decodeServerStatus(await this.request("server_status"));
  }

  subscribe(onChange: () => void): () => void {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void this.listen("todo:changed", onChange)
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }

  private async request(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> {
    try {
      return await this.invoke(command, args);
    } catch (error) {
      throw commandError(error);
    }
  }
}
