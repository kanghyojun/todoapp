export type Status = "todo" | "in_progress" | "done" | "deferred";

export type Priority = "none" | "urgent" | "high" | "medium" | "low";

export interface LinearRef {
  identifier: string;
  url: string;
}

export interface EmailRef {
  account_id: string;
  gmail_id: string;
  thread_id: string;
  subject: string;
  from_name: string;
  from_email: string;
}

export interface LinearStatus {
  configured: boolean;
  keyStoreAvailable: boolean;
  failing: number;
}

export interface Todo {
  id: string;
  /** 사람이 부르는 짧은 코드(4글자). 화면에는 #code 로 보인다. */
  code: string;
  title: string;
  description: string;
  status: Status;
  priority: Priority;
  due_date: string | null;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
  deferred_until: string | null;
  linear: LinearRef | null;
  email: EmailRef | null;
}

export interface Filter {
  status?: Status;
  priority?: Priority;
  due_before?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

export interface NewTodo {
  title: string;
  description?: string;
  status?: Status;
  priority?: Priority;
  due_date?: string;
}

export interface TodoPatch {
  title?: string;
  description?: string;
  status?: Status;
  priority?: Priority;
  due_date?: string | null;
}

export interface EmailLinkRequest {
  title: string;
  account_id: string;
  gmail_id: string;
  thread_id: string;
  subject: string;
  from_name: string;
  from_email: string;
}

export interface PullResult {
  created: number;
  skipped: number;
  completed?: number;
}
