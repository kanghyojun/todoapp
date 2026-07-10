import type { TodoClient, InvokeFn, ListenFn } from "./client";
import { HttpClient, TauriClient } from "./client";

interface TauriApi {
  invoke: InvokeFn;
  listen: ListenFn;
}

type LoadTauriApi = () => Promise<TauriApi>;

async function loadTauriApi(): Promise<TauriApi> {
  const [{ invoke }, { listen }] = await Promise.all([
    import("@tauri-apps/api/core"),
    import("@tauri-apps/api/event"),
  ]);
  return {
    invoke,
    listen: (event, handler) => listen(event, handler),
  };
}

export async function pickClient(
  tauri: boolean,
  token: string | undefined,
  loadApi: LoadTauriApi = loadTauriApi,
): Promise<TodoClient | null> {
  if (!tauri) {
    const normalizedToken = token?.trim();
    return normalizedToken ? new HttpClient(normalizedToken) : null;
  }
  const api = await loadApi();
  return new TauriClient(api.invoke, api.listen);
}
