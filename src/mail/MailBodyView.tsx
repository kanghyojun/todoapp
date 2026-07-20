import { For, Show, type Component } from "solid-js";
import type { MailBody } from "./domain";
import { linkifyText } from "./linkify";
import { prepareMailHtml } from "./mail-html";

interface MailBodyViewProps {
  // 아직 안 받아왔으면 null. 받아왔는데 두 파트가 다 비면 "본문이 없습니다".
  body: MailBody | null;
  // 본문 안의 링크를 눌렀을 때. 앱 창에서 열지 않고 외부 브라우저로 넘긴다.
  onOpenLink: (url: string) => void;
}

// 메일 본문 렌더러. HTML 파트를 먼저 쓴다. 실제 메일은 대부분
// multipart/alternative 라 text 파트가 서식과 표를 잃은 축약본이고,
// 아예 비어 있는 경우도 많아 원문과 다르게 보인다.
//
// 스크립트를 막으려고 sandbox 인 iframe 에 srcdoc 으로 넣는다. 허용한 건
// 사용자가 직접 누른 최상위 이동뿐이라 메일 안의 스크립트는 여전히 안 돈다.
// 그래서 별도 새니타이저는 두지 않았다. allow-scripts 를 열 일이 생기면
// 그때는 새니타이저가 필요하다.
//
// 메일 탭과 할 일 탭 미리보기가 같은 마크업을 쓰도록 한 곳에 모았다.
export const MailBodyView: Component<MailBodyViewProps> = (props) => (
  <div class="mail-body">
    <Show when={props.body} fallback={<p class="mail-loading">본문을 불러오는 중…</p>}>
      {(loaded) => (
        <Show
          when={loaded().body_html}
          fallback={
            <Show when={loaded().body_text} fallback={<p>본문이 없습니다.</p>}>
              {(text) => (
                <pre class="mail-body-text">
                  <For each={linkifyText(text())}>
                    {(segment) =>
                      segment.kind === "link" ? (
                        <a
                          class="mail-body-link"
                          href={segment.value}
                          onClick={(event) => {
                            event.preventDefault();
                            props.onOpenLink(segment.value);
                          }}
                        >
                          {segment.value}
                        </a>
                      ) : (
                        segment.value
                      )
                    }
                  </For>
                </pre>
              )}
            </Show>
          }
        >
          {(html) => (
            <iframe
              class="mail-body-html"
              sandbox="allow-top-navigation-by-user-activation"
              referrerpolicy="no-referrer"
              srcdoc={prepareMailHtml(html())}
              title="메일 본문"
            />
          )}
        </Show>
      )}
    </Show>
  </div>
);
