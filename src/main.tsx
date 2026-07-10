import { render } from "solid-js/web";
import { App } from "./App";
import { HttpClient } from "./client";
import "./styles.css";

const root = document.getElementById("root");

if (root === null) {
  throw new Error("missing #root element");
}

const token = import.meta.env.VITE_TODO_TOKEN?.trim();

if (token === undefined || token === "") {
  render(
    () => (
      <main class="setup-message" role="alert">
        <p>VITE_TODO_TOKEN이 없습니다.</p>
        <p>개발 서버를 시작하기 전에 다음 명령을 실행하십시오.</p>
        <code>export VITE_TODO_TOKEN=$(cat ~/.config/todo/token)</code>
      </main>
    ),
    root,
  );
} else {
  const client = new HttpClient(token);
  render(() => <App client={client} />, root);
}
