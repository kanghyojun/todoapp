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
  apiBase?: string,
): Promise<TodoClient | null> {
  if (!tauri) {
    const normalizedToken = token?.trim();
    if (!normalizedToken) return null;
    const base = apiBase?.trim();
    // 개발 중 다른 포트의 서버로 붙이고 싶으면 VITE_TODO_API 로 덮는다.
    return base ? new HttpClient(normalizedToken, base) : new HttpClient(normalizedToken);
  }
  const api = await loadApi();
  return new TauriClient(api.invoke, api.listen);
}
