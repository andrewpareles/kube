// The game reads keys and the mouse straight from the OS, before this page sees them, so the page never
// passes them on. It only keeps the browser from acting on them, like Space pressing a button, the arrow
// keys scrolling, or a press selecting text. The game hides the pointer itself while the camera turns.

// Elements that keep presses and scrolls to themselves. The chat marks itself with data-ui-only.
export const UI_ONLY_SELECTOR = "button, input, [data-ui-only]";

const isTextInput = (target: EventTarget | null) => target instanceof HTMLInputElement;

export const canScroll = (element: Element) => {
  const { overflowX, overflowY } = getComputedStyle(element);
  const canScrollX = (overflowX === "auto" || overflowX === "scroll") && element.scrollWidth > element.clientWidth;
  const canScrollY = (overflowY === "auto" || overflowY === "scroll") && element.scrollHeight > element.clientHeight;
  return canScrollX || canScrollY;
};

// The page never acts on keys itself unless the chat is being typed in.
const onKeyDown = (event: KeyboardEvent) => {
  if (isTextInput(event.target)) {
    return;
  }
  event.preventDefault();
};

// Presses never move focus or start selecting text, so the only thing that can hold focus is the chat.
const onMouseDown = (event: MouseEvent) => {
  if (!isTextInput(event.target)) {
    event.preventDefault();
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  }
};

// A pinch arrives as a scroll with ctrlKey, which would zoom the page.
const onWheel = (event: WheelEvent) => {
  if (event.ctrlKey) {
    event.preventDefault();
  }
};

export const startHandlingPageInput = () => {
  document.addEventListener("keydown", onKeyDown);
  document.addEventListener("mousedown", onMouseDown);
  document.addEventListener("wheel", onWheel, { passive: false });
  document.addEventListener("contextmenu", (event) => event.preventDefault());
};
