import { describe, expect, it } from "vitest";
import { HttpClient, NotFoundError } from "./client";

// 진짜 서버에 대고 도는 계약 검증. mock fetch 로는 wire 불일치를 못 잡는다.
// 서버를 띄우고 아래 두 환경변수를 주면 돈다. 없으면 건너뛴다.
//   cargo run -p todo-server -- --port 2477
//   TODO_E2E_TOKEN=$(cat ~/.config/todo/token) TODO_E2E_BASE=http://127.0.0.1:2477/api/v1
declare const process: { env: Record<string, string | undefined> };

const token = process.env.TODO_E2E_TOKEN;
const base = process.env.TODO_E2E_BASE;
const c = new HttpClient(token ?? "", base);

describe.skipIf(!token || !base)("HttpClient \u2194 \uc9c4\uc9dc todo-server", () => {
  it("생성/조회/수정/검색/삭제/복구가 실제 서버와 맞는다", async () => {
    const made = await c.create({ title: "실서버 왕복", priority: "urgent", due_date: "tomorrow" });
    expect(made.priority).toBe("urgent");           // 정수가 아니라 문자열
    expect(made.due_date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(made.status).toBe("todo");

    const got = await c.get(made.id);
    expect(got.title).toBe("실서버 왕복");

    // 제목과 상태를 한 번에 (M2.5 의 원자적 PATCH)
    const patched = await c.update(made.id, { title: "고침", status: "done" });
    expect(patched.title).toBe("고침");
    expect(patched.status).toBe("done");
    expect(patched.completed_at).toBeTruthy();

    // 검색 + 필터 동시 (M2.5)
    const found = await c.list({ q: "고침", status: "done" });
    expect(found.map((t) => t.id)).toContain(made.id);

    // 사람이 치는 문자로 검색해도 안 터진다
    for (const q of ['"', "*", "NOT", "(abc", '"고침"']) {
      await expect(c.list({ q })).resolves.toBeInstanceOf(Array);
    }

    // 소프트 삭제 -> 모든 경로 404
    await c.remove(made.id);
    await expect(c.get(made.id)).rejects.toBeInstanceOf(NotFoundError);
    await expect(c.update(made.id, { title: "x" })).rejects.toBeInstanceOf(NotFoundError);

    const back = await c.restore(made.id);
    expect(back.title).toBe("고침");
  });

  it("파싱 못 하는 날짜는 서버 메시지를 그대로 올린다", async () => {
    await expect(c.create({ title: "x", due_date: "내일쯤" })).rejects.toThrow(/invalid due date/);
  });

  it("setStatus 가 동작하고 없는 id 는 NotFound", async () => {
    const t = await c.create({ title: "상태 바꾸기" });
    const s = await c.setStatus(t.id, "in_progress");
    expect(s.status).toBe("in_progress");
    await expect(c.setStatus("019f0000-0000-7000-8000-000000000000", "done"))
      .rejects.toBeInstanceOf(NotFoundError);
  });

  it("Linear 경로는 501 을 던지되 크래시하지 않는다", async () => {
    const t = await c.create({ title: "링크 대상" });
    await expect(c.linkLinear(t.id, "PI-1234")).rejects.toThrow(/not implemented/i);
    await expect(c.pullLinear()).rejects.toThrow(/not implemented/i);
  });
});
