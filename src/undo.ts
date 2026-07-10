import type { Priority, Status } from "./domain";

export type InverseAction =
  | { type: "SetStatus"; id: string; status: Status }
  | { type: "SetPriority"; id: string; priority: Priority }
  | { type: "Restore"; id: string }
  | { type: "Remove"; id: string };

export type UndoEntry = readonly InverseAction[];

export class UndoStack {
  private readonly entries: UndoEntry[] = [];

  push(entry: UndoEntry): void {
    if (entry.length === 0) {
      return;
    }
    this.entries.push(entry);
    if (this.entries.length > 20) {
      this.entries.shift();
    }
  }

  pop(): UndoEntry | undefined {
    return this.entries.pop();
  }

  get size(): number {
    return this.entries.length;
  }
}
