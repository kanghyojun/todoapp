import { render } from "solid-js/web";
import { isTauri } from "@tauri-apps/api/core";
import { App } from "./App";
import { TauriClient } from "./client";
import type { GmailClient } from "./mail/client";
import { TauriGmailClient } from "./mail/client";
import { pickClient } from "./startup";
import { applyPreference, readPreference } from "./theme";
import "./styles.css";

// 렌더보다 먼저 건다. 그래야 저장된 테마와 OS 테마가 한 번 깜빡이지 않는다.
applyPreference(document.documentElement, readPreference(localStorage));

const root = document.getElementById("root");

if (root === null) {
  throw new Error("missing #root element");
}

async function start(mount: HTMLElement): Promise<void> {
  const tauri = isTauri();
  const client = await pickClient(
    tauri,
    tauri ? undefined : import.meta.env.VITE_TODO_TOKEN,
    undefined,
    tauri ? undefined : import.meta.env.VITE_TODO_API,
  );
  if (client === null) {
    render(
      () => (
        <main class="setup-message" role="alert">
          <p>VITE_TODO_TOKEN이 없습니다.</p>
          <p>개발 서버를 시작하기 전에 다음 명령을 실행하십시오.</p>
          <code>export VITE_TODO_TOKEN=$(cat ~/.config/todo/token)</code>
        </main>
      ),
      mount,
    );
    return;
  }

  let externalServerError: string | undefined;
  let gmailClient: GmailClient | undefined;
  if (client instanceof TauriClient) {
    try {
      const status = await client.serverStatus();
      if (!status.running) {
        externalServerError = `외부 인터페이스를 열지 못했습니다. ${status.error ?? "원인을 알 수 없습니다."}`;
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      externalServerError = `외부 인터페이스 상태를 읽지 못했습니다. ${message}`;
    }
    // 메일은 OAuth·키체인 때문에 데스크톱(Tauri) 전용이다.
    const [{ invoke }, { listen }] = await Promise.all([
      import("@tauri-apps/api/core"),
      import("@tauri-apps/api/event"),
    ]);
    gmailClient = new TauriGmailClient(invoke, (event, handler) =>
      listen(event, handler),
    );
  }
  render(
    () => (
      <App
        client={client}
        gmailClient={gmailClient}
        externalServerError={externalServerError}
      />
    ),
    mount,
  );
}

void start(root);
