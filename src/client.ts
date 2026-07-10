import type {
  Filter,
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
  linkLinear(id: string, issueRef: string): Promise<void>;
  pullLinear(): Promise<PullResult>;
  subscribe(onChange: () => void): () => void;
}

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

const STATUSES: readonly Status[] = ["todo", "in_progress", "done"];
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
    typeof value.title !== "string" ||
    typeof value.description !== "string" ||
    !STATUSES.includes(value.status as Status) ||
    !PRIORITIES.includes(value.priority as Priority) ||
    !isNullableString(value.due_date) ||
    !isNullableString(value.completed_at) ||
    typeof value.created_at !== "string" ||
    typeof value.updated_at !== "string" ||
    !isNullableString(value.deleted_at)
  ) {
    throw new ApiError("server returned an invalid todo", "invalid_response", 0);
  }
  return {
    id: value.id,
    title: value.title,
    description: value.description,
    status: value.status as Status,
    priority: value.priority as Priority,
    due_date: value.due_date,
    completed_at: value.completed_at,
    created_at: value.created_at,
    updated_at: value.updated_at,
    deleted_at: value.deleted_at,
  };
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

export class HttpClient implements TodoClient {
  constructor(
    private readonly token: string,
    private readonly baseUrl = "http://127.0.0.1:2470/api/v1",
    private readonly fetcher: Fetcher = fetch,
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

  async linkLinear(id: string, issueRef: string): Promise<void> {
    await this.request(`/todos/${encodeURIComponent(id)}/link/linear`, {
      method: "POST",
      body: JSON.stringify({ issue_ref: issueRef }),
    });
  }

  async pullLinear(): Promise<PullResult> {
    return decodePullResult(
      await this.request("/linear/pull", { method: "POST" }),
    );
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
