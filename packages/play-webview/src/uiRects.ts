import { postToHost, type UiRect } from "./hostBridge";
import { canScroll, UI_ONLY_SELECTOR } from "./pageInput";

// The host decides which presses and scrolls the game gets the moment they arrive, without asking the page.
// So each frame the page measures where its UI is and sends it to the host when anything moved: every
// element that keeps presses to itself, and every panel that can scroll. A rectangle is cut down to the part
// that can be seen, since a button scrolled out of its panel cannot be pressed.

// Script GUIs style everything with classes, so only elements with an overflow class can hide their insides.
const OVERFLOW_SELECTOR = '[class*="overflow"]';

const getVisibleRect = (element: Element): UiRect | null => {
  const { pointerEvents, visibility } = getComputedStyle(element);
  if (pointerEvents === "none" || visibility === "hidden") {
    return null;
  }
  const rect = element.getBoundingClientRect();
  let left = Math.max(rect.left, 0);
  let top = Math.max(rect.top, 0);
  let right = Math.min(rect.right, window.innerWidth);
  let bottom = Math.min(rect.bottom, window.innerHeight);
  for (let ancestor = element.parentElement?.closest(OVERFLOW_SELECTOR); ancestor; ancestor = ancestor.parentElement?.closest(OVERFLOW_SELECTOR)) {
    const { overflowX, overflowY } = getComputedStyle(ancestor);
    if (overflowX === "visible" && overflowY === "visible") {
      continue;
    }
    const ancestorRect = ancestor.getBoundingClientRect();
    left = Math.max(left, ancestorRect.left);
    top = Math.max(top, ancestorRect.top);
    right = Math.min(right, ancestorRect.right);
    bottom = Math.min(bottom, ancestorRect.bottom);
  }
  if (right <= left || bottom <= top) {
    return null;
  }
  const { devicePixelRatio } = window;
  return { x: left * devicePixelRatio, y: top * devicePixelRatio, width: (right - left) * devicePixelRatio, height: (bottom - top) * devicePixelRatio };
};

const getVisibleRects = (elements: Element[]) =>
  elements.flatMap((element) => {
    const visibleRect = getVisibleRect(element);
    return visibleRect === null ? [] : [visibleRect];
  });

const isSameRects = (rects: UiRect[], otherRects: UiRect[]) =>
  rects.length === otherRects.length &&
  rects.every(({ x, y, width, height }, rectIndex) => {
    const otherRect = otherRects[rectIndex];
    return x === otherRect.x && y === otherRect.y && width === otherRect.width && height === otherRect.height;
  });

export const startReportingUiRectsToHost = () => {
  let sentUiOnlyRects: UiRect[] = [];
  let sentScrollableRects: UiRect[] = [];
  const reportUiRects = () => {
    const uiOnlyRects = getVisibleRects(Array.from(document.querySelectorAll(UI_ONLY_SELECTOR)));
    const scrollableRects = getVisibleRects(Array.from(document.querySelectorAll(OVERFLOW_SELECTOR)).filter(canScroll));
    if (!isSameRects(uiOnlyRects, sentUiOnlyRects) || !isSameRects(scrollableRects, sentScrollableRects)) {
      postToHost({ setUiRects: { uiOnlyRects, scrollableRects } });
      sentUiOnlyRects = uiOnlyRects;
      sentScrollableRects = scrollableRects;
    }
    requestAnimationFrame(reportUiRects);
  };
  requestAnimationFrame(reportUiRects);
};
