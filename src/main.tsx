import { render } from "solid-js/web";
import { isTauri } from "@tauri-apps/api/core";
import { App } from "./App";
import { TauriClient } from "./client";
import { pickClient } from "./startup";
import "./styles.css";

const root = document.getElementById("root");

if (root === null) {
  throw new Error("missing #root element");
}

async function start(mount: HTMLElement): Promise<void> {
  const tauri = isTauri();
  const client = await pickClient(
    tauri,
    tauri ? undefined : import.meta.env.VITE_TODO_TOKEN,
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
  }
  render(
    () => <App client={client} externalServerError={externalServerError} />,
    mount,
  );
}

void start(root);
