export type Status = "todo" | "in_progress" | "done";

export type Priority = "none" | "urgent" | "high" | "medium" | "low";

export interface LinearRef {
  identifier: string;
  url: string;
}

export interface LinearStatus {
  configured: boolean;
  keyStoreAvailable: boolean;
  failing: number;
}

export interface Todo {
  id: string;
  title: string;
  description: string;
  status: Status;
  priority: Priority;
  due_date: string | null;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
  linear: LinearRef | null;
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

export interface PullResult {
  created: number;
  skipped: number;
  completed?: number;
}
