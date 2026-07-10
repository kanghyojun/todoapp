import { describe, expect, it } from "vitest";
import { UndoStack, type InverseAction } from "./undo";

describe("UndoStack", () => {
  it("maps delete to restore and create to remove", () => {
    const stack = new UndoStack();
    stack.push([{ type: "Restore", id: "deleted" }]);
    expect(stack.pop()).toEqual([{ type: "Restore", id: "deleted" }]);
    stack.push([{ type: "Remove", id: "created" }]);
    expect(stack.pop()).toEqual([{ type: "Remove", id: "created" }]);
  });

  it("restores the status from before the second done toggle", () => {
    const stack = new UndoStack();
    let status: "todo" | "done" = "todo";
    const toggle = (): void => {
      const previous = status;
      status = status === "done" ? "todo" : "done";
      stack.push([{ type: "SetStatus", id: "a", status: previous }]);
    };
    toggle();
    toggle();
    expect(status).toBe("todo");
    expect(stack.pop()).toEqual([{ type: "SetStatus", id: "a", status: "done" }]);
  });

  it("caps the history at 20 entries", () => {
    const stack = new UndoStack();
    for (let index = 0; index < 21; index += 1) {
      const action: InverseAction = { type: "Restore", id: String(index) };
      stack.push([action]);
    }
    expect(stack.size).toBe(20);
    const ids: string[] = [];
    let entry = stack.pop();
    while (entry !== undefined) {
      const action = entry[0];
      if (action !== undefined) {
        ids.push(action.id);
      }
      entry = stack.pop();
    }
    expect(ids.at(-1)).toBe("1");
  });
});
