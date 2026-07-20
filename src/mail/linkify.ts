export type TextSegment =
  | { kind: "text"; value: string }
  | { kind: "link"; value: string };

// 공백과 꺾쇠·따옴표 전까지를 URL 후보로 본다. 뒤에 붙은 문장부호는
// trimTrailing 이 떼어낸다.
const URL_PATTERN = /https?:\/\/[^\s<>"']+/gi;

const TRAILING_PUNCTUATION = ".,;:!?'\"";

function countChar(text: string, target: string): number {
  let count = 0;
  for (const char of text) if (char === target) count += 1;
  return count;
}

// "https://a.com." 의 마침표까지 URL 로 먹으면 링크가 안 열린다.
// 다만 짝이 맞는 괄호는 남긴다. 위키 주소처럼 ")" 로 끝나는 URL 이 있다.
function trimTrailing(url: string): string {
  let out = url;
  while (out.length > 0) {
    const last = out[out.length - 1]!;
    if (TRAILING_PUNCTUATION.includes(last)) {
      out = out.slice(0, -1);
      continue;
    }
    if (last === ")" && countChar(out, ")") > countChar(out, "(")) {
      out = out.slice(0, -1);
      continue;
    }
    break;
  }
  return out;
}

// plain text 본문을 텍스트 조각과 링크 조각으로 가른다. 텍스트 메일에는
// URL 이 그냥 문자열로 박혀 있어 그대로 두면 클릭할 수가 없다.
export function linkifyText(text: string): TextSegment[] {
  const segments: TextSegment[] = [];
  let consumed = 0;
  for (const match of text.matchAll(URL_PATTERN)) {
    const start = match.index;
    const url = trimTrailing(match[0]);
    // 스킴만 남았으면 링크가 아니다. 뒤따르는 텍스트 조각에 그대로 섞인다.
    if (!/^https?:\/\/[^\s]/i.test(url)) continue;
    if (start > consumed) {
      segments.push({ kind: "text", value: text.slice(consumed, start) });
    }
    segments.push({ kind: "link", value: url });
    consumed = start + url.length;
  }
  if (consumed < text.length) {
    segments.push({ kind: "text", value: text.slice(consumed) });
  }
  return segments;
}
