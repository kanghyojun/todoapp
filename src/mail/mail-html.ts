// 본문 iframe 의 srcdoc 앞머리에 끼워 넣는 머리말.
//
// base target="_top": iframe 은 sandbox 라 스크립트가 안 돈다. 안에서 바깥으로
// 신호를 보낼 길이 없어서, 링크 클릭을 최상위 이동으로 올려 Rust 쪽
// on_navigation 이 잡아 외부 브라우저로 넘기게 한다.
//
// CSP frame-src: on_navigation 은 최상위 이동만 받는 게 아니라 iframe 이동도
// 전부 받는다(wry 에 main frame 필터가 없다). 메일이 중첩 iframe 을 달고 있으면
// 메일을 여는 것만으로 그 주소가 브라우저에 열려 버린다. 중첩 프레임을 아예
// 막아서 그 경로를 없앤다.
//
// default-src 는 일부러 안 건다. 걸면 이미지와 스타일까지 막혀 본문이 깨진다.
// 기본 폰트: iframe 문서에는 스타일이 하나도 없어서 브라우저 기본 serif 가
// 잡힌다. 한글은 그게 명조/궁서 계열로 떨어져 읽기 불편하다. 앱이 본문
// 텍스트에 쓰는 값(.description 과 같은 14px/1.65 system-ui)에 맞춘다.
//
// html 에만 건다. 메일이 자기 font-family 를 갖고 있으면 그쪽이 이겨야 한다.
// 이건 메일에 아무 지정이 없을 때 채우는 바닥값이다.
const DEFAULT_STYLE = "<style>html{font:14px/1.65 system-ui,sans-serif}</style>";

const PREAMBLE =
  '<meta http-equiv="Content-Security-Policy" content="script-src \'none\'; frame-src \'none\'; child-src \'none\'; object-src \'none\'; form-action \'none\'">' +
  '<base target="_top">' +
  DEFAULT_STYLE;

const LEADING_DOCTYPE = /^\s*<!doctype[^>]*>/i;

// 머리말을 doctype 뒤에 넣는다. doctype 앞에 뭔가 있으면 문서가 quirks mode 로
// 떨어져 메일 레이아웃이 원문과 다르게 잡힌다.
export function prepareMailHtml(html: string): string {
  const doctype = LEADING_DOCTYPE.exec(html);
  if (doctype === null) return `${PREAMBLE}${html}`;
  const end = doctype[0].length;
  return `${html.slice(0, end)}${PREAMBLE}${html.slice(end)}`;
}
