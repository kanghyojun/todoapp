import type { MailFolder } from "./domain";

export type MailKeyAction =
  | { type: "Move"; delta: number }
  | { type: "Open" }
  | { type: "Close" }
  | { type: "Archive" }
  | { type: "ToggleRead" }
  | { type: "SetFolder"; folder: MailFolder }
  | { type: "OpenSearch" };

export interface MailKeyState {
  focus: "text" | "other";
  detailOpen: boolean;
}

// Superhuman 식 트리아지 단축키. ⌘K/? 및 탭 전환은 전역(App)에서 처리한다.
export function handleMailKey(
  state: MailKeyState,
  key: string,
): MailKeyAction | null {
  // Escape 는 검색 입력 중(text 포커스)에도 닫혀야 하므로 포커스 가드보다 앞에 둔다.
  if (key === "Escape") {
    return { type: "Close" };
  }
  if (state.focus === "text") {
    return null;
  }
  switch (key) {
    case "j":
      return { type: "Move", delta: 1 };
    case "k":
      return { type: "Move", delta: -1 };
    case "Enter":
      return { type: "Open" };
    case "e":
      return { type: "Archive" };
    case "u":
      return { type: "ToggleRead" };
    case "1":
      return { type: "SetFolder", folder: "inbox" };
    case "2":
      return { type: "SetFolder", folder: "archive" };
    case "3":
      return { type: "SetFolder", folder: "all" };
    case "/":
      return { type: "OpenSearch" };
    default:
      return null;
  }
}
