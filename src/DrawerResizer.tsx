import { onCleanup, type Component } from "solid-js";
import {
  DEFAULT_DRAWER_WIDTH,
  DRAWER_WIDTH_VARS,
  clampDrawerWidth,
  readDrawerWidth,
  saveDrawerWidth,
  widthFromPointer,
  type DrawerKind,
} from "./drawer-width";

interface DrawerResizerProps {
  kind: DrawerKind;
}

function applyWidth(kind: DrawerKind, width: number): void {
  document.documentElement.style.setProperty(
    DRAWER_WIDTH_VARS[kind],
    `${width}px`,
  );
}

// 시작할 때 저장된 폭을 CSS 변수에 심는다. 창 크기에 맞게 잘라서 넣으므로
// 넓은 화면에서 저장한 값이 좁은 화면을 다 덮지 않는다.
export function restoreDrawerWidths(): void {
  for (const kind of ["todo", "mail"] as const) {
    applyWidth(kind, readDrawerWidth(localStorage, kind, window.innerWidth));
  }
}

// 드로어 왼쪽 가장자리에 세우는 드래그 손잡이.
//
// 폭은 시그널이 아니라 CSS 변수로 나간다. 드래그 중에는 프레임마다 값이
// 바뀌는데, 시그널로 두면 그때마다 드로어 안쪽이 통째로 다시 그려진다.
// 변수만 갈면 레이아웃만 다시 잡힌다.
export const DrawerResizer: Component<DrawerResizerProps> = (props) => {
  let dragging = false;

  function onPointerMove(event: PointerEvent): void {
    if (!dragging) return;
    applyWidth(props.kind, widthFromPointer(event.clientX, window.innerWidth));
  }

  function stopDragging(): void {
    if (!dragging) return;
    dragging = false;
    document.body.classList.remove("drawer-resizing");
    const current = document.documentElement.style.getPropertyValue(
      DRAWER_WIDTH_VARS[props.kind],
    );
    const width = Number.parseInt(current, 10);
    if (!Number.isNaN(width)) {
      saveDrawerWidth(localStorage, props.kind, width);
    }
  }

  // 창을 줄이면 저장된 폭이 화면보다 넓어질 수 있다. 그때 다시 잘라 준다.
  function onResize(): void {
    const current = document.documentElement.style.getPropertyValue(
      DRAWER_WIDTH_VARS[props.kind],
    );
    const width = Number.parseInt(current, 10);
    if (Number.isNaN(width)) return;
    applyWidth(props.kind, clampDrawerWidth(width, window.innerWidth));
  }

  window.addEventListener("resize", onResize);
  onCleanup(() => window.removeEventListener("resize", onResize));

  return (
    <div
      class="drawer-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="드로어 너비 조절"
      onPointerDown={(event) => {
        // 주 버튼만. 오른쪽 클릭으로 끌리면 놓는 순간을 못 잡는다.
        if (event.button !== 0) return;
        event.preventDefault();
        dragging = true;
        document.body.classList.add("drawer-resizing");
        // 포인터를 잡아 둬야 드로어 밖으로 끌고 나가도 이벤트가 계속 온다.
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={onPointerMove}
      onPointerUp={stopDragging}
      onPointerCancel={stopDragging}
      onDblClick={() => {
        applyWidth(props.kind, DEFAULT_DRAWER_WIDTH[props.kind]);
        saveDrawerWidth(localStorage, props.kind, DEFAULT_DRAWER_WIDTH[props.kind]);
      }}
    />
  );
};
