import { describe, expect, it, vi } from "vitest";
import { HttpClient } from "./client";
import { pickClient } from "./startup";

describe("pickClient", () => {
  it("chooses HttpClient outside Tauri and does not load the Tauri runtime", async () => {
    const loadApi = vi.fn();

    const client = await pickClient(false, "development-token", loadApi);

    expect(client).toBeInstanceOf(HttpClient);
    expect(loadApi).not.toHaveBeenCalled();
  });
});
