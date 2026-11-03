import type { MouseEvent } from "react";
import type { MouseButtonName } from "../../kube-ts-api/src/clientInputService";
import type { GuiSerializedElementNode, GuiSerializedNode } from "../../kube-ts-api/src/gui/guiTypes";
import { postToHost, type ScriptGuiRoot } from "./hostBridge";

// Client scripts render their GUI with kube-ts-api's own JSX runtime, which sends the finished tree
// here. React turns that tree into real divs and buttons with the scripts' own class names, and the
// scripts' click and hover handlers are called back through the host. Each root covers the whole screen,
// and scripts place their content in it with Tailwind.

type ScriptGuiKey = { scriptObjectId: number; guiKey: string };

// Elements name their right click and mouse button handlers in these attributes, so each root can find the
// innermost element under the pointer that has one.
const ON_RIGHT_CLICK_ATTRIBUTE = "data-on-right-click";
const ON_MOUSE_DOWN_ATTRIBUTE = "data-on-mouse-down";
const ON_MOUSE_UP_ATTRIBUTE = "data-on-mouse-up";

const getNodeId = (node: GuiSerializedNode) => ("text" in node ? node.text.nodeId : node.element.nodeId);

const GuiElementView = ({ scriptGuiKey, element }: { scriptGuiKey: ScriptGuiKey; element: GuiSerializedElementNode["element"] }) => {
  const { elementKind, className, onClickHandlerId, onRightClickHandlerId, onMouseDownHandlerId, onMouseUpHandlerId, onHoverHandlerId, children } = element;
  // Only the innermost element with a click handler gets the click.
  const onClick =
    onClickHandlerId === null
      ? undefined
      : (event: MouseEvent) => {
          event.stopPropagation();
          postToHost({ guiHandler: { ...scriptGuiKey, handlerId: onClickHandlerId, button: null } });
        };
  const onPointerEnter = onHoverHandlerId === null ? undefined : () => postToHost({ guiHandler: { ...scriptGuiKey, handlerId: onHoverHandlerId, button: null } });
  // An element with a click or mouse button handler keeps presses from the game, like a button does.
  const shouldKeepPresses = onClickHandlerId !== null || onRightClickHandlerId !== null || onMouseDownHandlerId !== null || onMouseUpHandlerId !== null;
  const attributes = {
    "data-ui-only": shouldKeepPresses ? "" : undefined,
    [ON_RIGHT_CLICK_ATTRIBUTE]: onRightClickHandlerId ?? undefined,
    [ON_MOUSE_DOWN_ATTRIBUTE]: onMouseDownHandlerId ?? undefined,
    [ON_MOUSE_UP_ATTRIBUTE]: onMouseUpHandlerId ?? undefined,
  };
  const childViews = children.map((child) => <GuiNodeView key={getNodeId(child)} scriptGuiKey={scriptGuiKey} node={child} />);
  if (elementKind === "button") {
    return (
      <button type="button" className={className} onClick={onClick} onPointerEnter={onPointerEnter} {...attributes}>
        {childViews}
      </button>
    );
  }
  return (
    <div className={className} onClick={onClick} onPointerEnter={onPointerEnter} {...attributes}>
      {childViews}
    </div>
  );
};

const GuiNodeView = ({ scriptGuiKey, node }: { scriptGuiKey: ScriptGuiKey; node: GuiSerializedNode }) => {
  if ("text" in node) {
    return node.text.text;
  }
  return <GuiElementView scriptGuiKey={scriptGuiKey} element={node.element} />;
};

// -------------------------- mouse buttons -------------------------------

// Scripts only hear about the left and right buttons, the same as their mouse listeners.
const getMouseButtonName = (button: number): MouseButtonName | null => {
  switch (button) {
    case 0:
      return "left";
    case 2:
      return "right";
    default:
      return null;
  }
};

const getInnermostHandlerId = (target: EventTarget, attribute: string) =>
  target instanceof Element ? (target.closest(`[${attribute}]`)?.getAttribute(attribute) ?? null) : null;

// The innermost element with a right click handler where the right button last went down, or null. The page sees
// every press in the window, even outside every GUI, so this always belongs to the latest press.
let rightMouseDownElement: Element | null = null;

export const startTrackingRightMouseDown = () => {
  document.addEventListener("mousedown", (event) => {
    if (event.button === 2) {
      rightMouseDownElement = event.target instanceof Element ? event.target.closest(`[${ON_RIGHT_CLICK_ATTRIBUTE}]`) : null;
    }
  });
};

// A right click goes to the innermost element with a right click handler that the button both went down and came back up in.
const getRightClickHandlerId = (target: EventTarget) => {
  if (!(target instanceof Node)) {
    return null;
  }
  let rightClickElement = rightMouseDownElement;
  while (rightClickElement !== null && !rightClickElement.contains(target)) {
    rightClickElement = rightClickElement.parentElement?.closest(`[${ON_RIGHT_CLICK_ATTRIBUTE}]`) ?? null;
  }
  return rightClickElement?.getAttribute(ON_RIGHT_CLICK_ATTRIBUTE) ?? null;
};

const onScriptGuiMouseDown = (scriptGuiKey: ScriptGuiKey, event: MouseEvent) => {
  const button = getMouseButtonName(event.button);
  const handlerId = getInnermostHandlerId(event.target, ON_MOUSE_DOWN_ATTRIBUTE);
  if (button !== null && handlerId !== null) {
    postToHost({ guiHandler: { ...scriptGuiKey, handlerId, button } });
  }
};

const onScriptGuiMouseUp = (scriptGuiKey: ScriptGuiKey, event: MouseEvent) => {
  const button = getMouseButtonName(event.button);
  if (button === null) {
    return;
  }
  const handlerId = getInnermostHandlerId(event.target, ON_MOUSE_UP_ATTRIBUTE);
  if (handlerId !== null) {
    postToHost({ guiHandler: { ...scriptGuiKey, handlerId, button } });
  }
  const rightClickHandlerId = button === "right" ? getRightClickHandlerId(event.target) : null;
  if (rightClickHandlerId !== null) {
    postToHost({ guiHandler: { ...scriptGuiKey, handlerId: rightClickHandlerId, button: null } });
  }
};

export const ScriptGuiRoots = ({ scriptGuiRoots }: { scriptGuiRoots: ScriptGuiRoot[] }) =>
  scriptGuiRoots.map(({ scriptObjectId, guiKey, root }) => (
    <div
      key={`${scriptObjectId}:${guiKey}`}
      data-script-gui-root=""
      className="pointer-events-none absolute inset-0"
      onMouseDown={(event) => onScriptGuiMouseDown({ scriptObjectId, guiKey }, event)}
      onMouseUp={(event) => onScriptGuiMouseUp({ scriptObjectId, guiKey }, event)}
    >
      {root.children.map((child) => (
        <GuiNodeView key={getNodeId(child)} scriptGuiKey={{ scriptObjectId, guiKey }} node={child} />
      ))}
    </div>
  ));
