import { denoOps } from "../object_types/__globalOps__";
import type { ObjectId } from "../object_types/shared";
import { getRustObject } from "../world";
import type {
  GuiComponent,
  GuiElementKind,
  GuiHandler,
  GuiJsxElement,
  GuiJsxProps,
  GuiMouseButtonHandler,
  GuiRenderable,
  GuiRenderedRoot,
  GuiRootRenderable,
  GuiSerializedNode,
} from "./guiTypes";




// this is the actual game.guiService runtime. jsx-runtime.ts is the tiny compiler hook that builds JSX nodes.
type GuiShownEntry = {
  rootRenderable: GuiRootRenderable;
};

type StateOfHookId = {
  [hookId: string]: unknown;
};

type NextStateValue<StateValue> =
  | StateValue
  | ((prevState: StateValue) => StateValue);

type CreatedStateOfStoreId = {
  [storeId: string]: CreatedStateInternal | undefined;
};

type GuiRootRef = {
  guiKey: string;
  scriptObjectId: ObjectId;
};

type CreatedStateInternal = {
  subscriberOfGuiRootKey: {
    [guiRootKey: string]: GuiRootRef | undefined;
  };
};

type GuiState = {
  shownEntry: GuiShownEntry | undefined;
  stateOfHookId: StateOfHookId | undefined;
  createdStateOfStoreIdReadLastRender: CreatedStateOfStoreId | undefined;
};

type GuiStateOfGuiKey = {
  [guiKey: string]: GuiState | undefined;
};

type GuiRenderSession = {
  componentPath: string;
  didSeeHookId: { [hookId: string]: true | undefined };
  createdStateOfStoreIdReadThisRender: CreatedStateOfStoreId;
  localHandlerOfHandlerId: { [handlerId: string]: GuiHandler | GuiMouseButtonHandler | undefined };
  scriptObjectId: ObjectId;
  guiKey: string;
};

const guiStateOfGuiKeyOfScriptObjectId: {
  [scriptObjectId: number]: GuiStateOfGuiKey | undefined;
} = Object.create(null);

let currentRenderSession: GuiRenderSession | null = null;
let currentHookIndex = 0;
let nextCreatedStateId = 0;

const createGuiStateOfGuiKey = (): GuiStateOfGuiKey => {
  return Object.create(null);
};

const createStateOfHookId = (): StateOfHookId => {
  return Object.create(null);
};

const createCreatedStateOfStoreId = (): CreatedStateOfStoreId => {
  return Object.create(null);
};

const createCreatedStateInternal = (): CreatedStateInternal => {
  return {
    subscriberOfGuiRootKey: Object.create(null),
  };
};

const createGuiState = (): GuiState => {
  return {
    shownEntry: undefined,
    stateOfHookId: undefined,
    createdStateOfStoreIdReadLastRender: undefined,
  };
};

const getBoundScriptObjectId = (scriptObjectId: ObjectId | undefined): ObjectId => {
  if (scriptObjectId === undefined) {
    throw new Error("game.guiService can only be used inside a running script.");
  }
  return scriptObjectId;
};

const guiStateOfGuiKeyForScriptObjectId = (scriptObjectId: ObjectId): GuiStateOfGuiKey => {
  let guiStateOfGuiKey = guiStateOfGuiKeyOfScriptObjectId[scriptObjectId];
  if (guiStateOfGuiKey === undefined) {
    guiStateOfGuiKey = createGuiStateOfGuiKey();
    guiStateOfGuiKeyOfScriptObjectId[scriptObjectId] = guiStateOfGuiKey;
  }
  return guiStateOfGuiKey;
};

const guiStateForScriptObjectIdAndGuiKey = (scriptObjectId: ObjectId, guiKey: string): GuiState => {
  const guiStateOfGuiKey = guiStateOfGuiKeyForScriptObjectId(scriptObjectId);
  let guiState = guiStateOfGuiKey[guiKey];
  if (guiState === undefined) {
    guiState = createGuiState();
    guiStateOfGuiKey[guiKey] = guiState;
  }
  return guiState;
};

const guiRootKey = (scriptObjectId: ObjectId, guiKey: string): string => {
  return `${scriptObjectId}:${guiKey}`;
};

// This removes all shared-state subscriptions for a root when it stops existing.
const clearGuiRootStoreSubscriptions = (createdStateOfStoreIdReadLastRender: CreatedStateOfStoreId | undefined, scriptObjectId: ObjectId, guiKey: string) => {
  const currentGuiRootKey = guiRootKey(scriptObjectId, guiKey);
  for (const storeId of Object.keys(createdStateOfStoreIdReadLastRender ?? {})) {
    const createdState = createdStateOfStoreIdReadLastRender?.[storeId];
    if (createdState === undefined) {
      continue;
    }
    delete createdState.subscriberOfGuiRootKey[currentGuiRootKey];
  }
};

// This keeps the per-script GUI state maps small when a mounted root is removed.
const clearGuiKeyState = (scriptObjectId: ObjectId, guiKey: string) => {
  const guiStateOfGuiKey = guiStateOfGuiKeyOfScriptObjectId[scriptObjectId];
  if (guiStateOfGuiKey !== undefined) {
    const guiState = guiStateOfGuiKey[guiKey];
    clearGuiRootStoreSubscriptions(guiState?.createdStateOfStoreIdReadLastRender, scriptObjectId, guiKey);
    delete guiStateOfGuiKey[guiKey];
    if (Object.keys(guiStateOfGuiKey).length === 0) {
      delete guiStateOfGuiKeyOfScriptObjectId[scriptObjectId];
    }
  }
};

// This keeps the Rust-side GUI storage in sync when a mounted root disappears.
const clearRustGui = (scriptObjectId: ObjectId, guiKey: string) => {
  denoOps.__clearRustGuiRoot__(scriptObjectId, guiKey);
};

const isGuiJsxElement = (value: unknown): value is GuiJsxElement => {
  if (value === null || typeof value !== "object") {
    return false;
  }
  return "tag" in value && "props" in value;
};

// This renders one component frame while keeping hook state scoped to that component path.
const renderComponent = (Component: GuiComponent<object>, props: GuiJsxProps, componentPath: string): GuiRenderable => {
  const previousRenderSession = currentRenderSession;
  const previousHookIndex = currentHookIndex;
  if (previousRenderSession === null) {
    throw new Error("GUI components can only render inside game.guiService.show().");
  }

  currentRenderSession = {
    ...previousRenderSession,
    componentPath,
  };
  currentHookIndex = 0;

  try {
    return Component(props);
  } finally {
    currentRenderSession = previousRenderSession;
    currentHookIndex = previousHookIndex;
  }
};

const renderRootComponent = (renderRoot: () => GuiRenderable): GuiRenderable => {
  const previousRenderSession = currentRenderSession;
  const previousHookIndex = currentHookIndex;
  if (previousRenderSession === null) {
    throw new Error("GUI components can only render inside game.guiService.show().");
  }

  currentRenderSession = {
    ...previousRenderSession,
    componentPath: "root.component",
  };
  currentHookIndex = 0;

  try {
    return renderRoot();
  } finally {
    currentRenderSession = previousRenderSession;
    currentHookIndex = previousHookIndex;
  }
};

// This walks the JSX tree and converts it into the plain serialized GUI tree that Rust stores.
const serializeRenderable = (renderable: GuiRenderable, path: string): GuiSerializedNode[] => {
  if (renderable === null || renderable === undefined || renderable === false) {
    return [];
  }
  if (renderable === true) {
    return [];
  }
  if (Array.isArray(renderable)) {
    const serializedNodes: GuiSerializedNode[] = [];
    for (let childIndex = 0; childIndex < renderable.length; childIndex += 1) {
      const childNodes = serializeRenderable(renderable[childIndex], `${path}.child${childIndex}`);
      for (const childNode of childNodes) {
        const previousNode = serializedNodes[serializedNodes.length - 1];
        if (previousNode !== undefined && "text" in previousNode && "text" in childNode) {
          previousNode.text.text += childNode.text.text;
        } else {
          serializedNodes.push(childNode);
        }
      }
    }
    return serializedNodes;
  }
  if (typeof renderable === "string" || typeof renderable === "number") {
    return [
      {
        text: {
          nodeId: `${path}.text`,
          text: `${renderable}`,
        },
      },
    ];
  }
  if (!isGuiJsxElement(renderable)) {
    throw new Error("GUI components can only render divs, buttons, components, text, arrays, or null.");
  }

  if (typeof renderable.tag === "function") {
    const componentName = renderable.tag.name || "AnonymousComponent";
    const nestedRenderable = renderComponent(renderable.tag, renderable.props, `${path}.component.${componentName}`);
    return serializeRenderable(nestedRenderable, path);
  }

  let elementKind: GuiElementKind;
  if (renderable.tag === "div") {
    elementKind = "div";
  } else if (renderable.tag === "button") {
    elementKind = "button";
  } else {
    throw new Error(
      `Only <div>, <button>, and component tags are supported in runtime GUI. Got <${renderable.tag}>.`,
    );
  }

  const renderSession = currentRenderSession;
  if (renderSession === null) {
    throw new Error("GUI elements can only render inside game.guiService.show().");
  }

  // The page sends back a handler's id when the player triggers it, and Rust calls the function stored under that id.
  const registerHandler = (handlerName: string, handler: GuiHandler | GuiMouseButtonHandler | undefined): string | null => {
    if (typeof handler !== "function") {
      return null;
    }
    const handlerId = `${path}.${handlerName}`;
    renderSession.localHandlerOfHandlerId[handlerId] = handler;
    return handlerId;
  };
  const onClickHandlerId = registerHandler("onClick", renderable.props.onClick);
  const onRightClickHandlerId = registerHandler("onRightClick", renderable.props.onRightClick);
  const onMouseDownHandlerId = registerHandler("onMouseDown", renderable.props.onMouseDown);
  const onMouseUpHandlerId = registerHandler("onMouseUp", renderable.props.onMouseUp);
  const onHoverHandlerId = registerHandler("onHover", renderable.props.onHover);

  return [
    {
      element: {
        elementKind,
        nodeId: `${path}.element`,
        className:
          typeof renderable.props.className === "string"
            ? renderable.props.className
            : "",
        onClickHandlerId,
        onRightClickHandlerId,
        onMouseDownHandlerId,
        onMouseUpHandlerId,
        onHoverHandlerId,
        children: serializeRenderable(renderable.props.children, `${path}.children`),
      },
    },
  ];
};

// This keeps each root subscribed only to the shared states it actually read last render.
const syncGuiRootStoreSubscriptions = (guiState: GuiState, renderSession: GuiRenderSession) => {
  const previousCreatedStateOfStoreId = guiState.createdStateOfStoreIdReadLastRender ?? createCreatedStateOfStoreId();
  const nextCreatedStateOfStoreId = renderSession.createdStateOfStoreIdReadThisRender;
  const currentGuiRootKey = guiRootKey(renderSession.scriptObjectId, renderSession.guiKey);
  const guiRootRef: GuiRootRef = {
    guiKey: renderSession.guiKey,
    scriptObjectId: renderSession.scriptObjectId,
  };

  for (const storeId of Object.keys(previousCreatedStateOfStoreId)) {
    if (nextCreatedStateOfStoreId[storeId] !== undefined) {
      continue;
    }
    const createdState = previousCreatedStateOfStoreId[storeId];
    if (createdState === undefined) {
      continue;
    }
    delete createdState.subscriberOfGuiRootKey[currentGuiRootKey];
  }

  for (const storeId of Object.keys(nextCreatedStateOfStoreId)) {
    const createdState = nextCreatedStateOfStoreId[storeId];
    if (createdState === undefined) {
      continue;
    }
    createdState.subscriberOfGuiRootKey[currentGuiRootKey] = guiRootRef;
  }

  guiState.createdStateOfStoreIdReadLastRender = nextCreatedStateOfStoreId;
};

// This re-renders one mounted GUI key and pushes the latest serialized tree into Rust.
const renderGui = (scriptObjectId: ObjectId, guiKey: string) => {
  if (getRustObject(scriptObjectId) === null) {
    clearGuiKeyState(scriptObjectId, guiKey);
    clearRustGui(scriptObjectId, guiKey);
    return;
  }

  const guiState = guiStateOfGuiKeyOfScriptObjectId[scriptObjectId]?.[guiKey];
  const shownEntry = guiState?.shownEntry;
  if (guiState === undefined || shownEntry === undefined) {
    clearGuiKeyState(scriptObjectId, guiKey);
    clearRustGui(scriptObjectId, guiKey);
    return;
  }

  const renderSession: GuiRenderSession = {
    componentPath: "root",
    didSeeHookId: Object.create(null),
    createdStateOfStoreIdReadThisRender: createCreatedStateOfStoreId(),
    localHandlerOfHandlerId: Object.create(null),
    scriptObjectId,
    guiKey,
  };
  const previousRenderSession = currentRenderSession;
  const previousHookIndex = currentHookIndex;
  currentRenderSession = renderSession;
  currentHookIndex = 0;

  let renderedChildren: GuiSerializedNode[] = [];
  try {
    const rootRenderable =
      typeof shownEntry.rootRenderable === "function"
        ? renderRootComponent(shownEntry.rootRenderable)
        : shownEntry.rootRenderable;
    renderedChildren = serializeRenderable(rootRenderable, "root");
  } finally {
    currentRenderSession = previousRenderSession;
    currentHookIndex = previousHookIndex;
  }

  const nextStateOfHookId: StateOfHookId = guiState.stateOfHookId ?? createStateOfHookId();
  for (const hookId of Object.keys(nextStateOfHookId)) {
    if (renderSession.didSeeHookId[hookId] === true) {
      continue;
    }
    delete nextStateOfHookId[hookId];
  }
  guiState.stateOfHookId = nextStateOfHookId;
  syncGuiRootStoreSubscriptions(guiState, renderSession);

  const renderedGuiRoot: GuiRenderedRoot = {
    children: renderedChildren,
  };
  denoOps.__clearRustGuiHandlers__(scriptObjectId, guiKey);
  for (const handlerId of Object.keys(renderSession.localHandlerOfHandlerId)) {
    const handler = renderSession.localHandlerOfHandlerId[handlerId];
    if (handler === undefined) {
      continue;
    }
    denoOps.__registerRustGuiHandler__(scriptObjectId, guiKey, handlerId, handler);
  }
  denoOps.__setRustGuiRoot__(scriptObjectId, guiKey, JSON.stringify(renderedGuiRoot));
};

/**
 * Only callable by a Client Script.
 *
 * The guiKey can be anything; it is a string you pick to identify the GUI root.
 */
export type GuiService = {
  /** Example: `game.guiService.show("my-key", <MyComponent />)` */
  show: (guiKey: string, rootRenderable: GuiRootRenderable) => void
  /** Example: `game.guiService.hide("my-key")` */
  hide: (guiKey: string) => void
}

export const createScriptGuiService = (scriptObjectId: ObjectId | undefined): GuiService => {
  return {
    hide: (guiKey: string) => {
      const boundScriptObjectId = getBoundScriptObjectId(scriptObjectId);
      clearGuiKeyState(boundScriptObjectId, guiKey);
      clearRustGui(boundScriptObjectId, guiKey);
    },

    show: (guiKey: string, rootRenderable: GuiRootRenderable) => {
      const boundScriptObjectId = getBoundScriptObjectId(scriptObjectId);
      const guiState = guiStateForScriptObjectIdAndGuiKey(boundScriptObjectId, guiKey);
      guiState.shownEntry = {
        rootRenderable,
      };
      renderGui(boundScriptObjectId, guiKey);
    },
  };
};

// -------------------------- shared gui state -------------------------------

/** Only callable by a Client Script. */
export const createState = <T,>(initialState: T): { get: () => T; set: (nextState: T | ((prevState: T) => T)) => void } => {
  const storeId = `${nextCreatedStateId}`;
  nextCreatedStateId += 1;

  let stateValue = initialState;
  const createdState = createCreatedStateInternal();

  return {
    get: () => {
      const renderSession = currentRenderSession;
      if (renderSession !== null) {
        renderSession.createdStateOfStoreIdReadThisRender[storeId] = createdState;
      }
      return stateValue;
    },

    set: (nextState: NextStateValue<T>) => {
      const nextStateValue =
        typeof nextState === "function"
          ? (nextState as (prevState: T) => T)(stateValue)
          : nextState;
      if (Object.is(stateValue, nextStateValue)) {
        return;
      }

      stateValue = nextStateValue;
      for (const currentGuiRootKey of Object.keys(createdState.subscriberOfGuiRootKey)) {
        const guiRootRef = createdState.subscriberOfGuiRootKey[currentGuiRootKey];
        if (guiRootRef === undefined) {
          continue;
        }
        renderGui(guiRootRef.scriptObjectId, guiRootRef.guiKey);
      }
    },
  };
};

// -------------------------- hook state -------------------------------

/** Only callable by a Client Script. */
export const useState = <T,>(initialState: T | (() => T)): [T, (nextState: T | ((prevState: T) => T)) => void] => {
  const renderSession = currentRenderSession;
  if (renderSession === null) {
    throw new Error("useState can only be called while rendering a GUI component.");
  }

  const hookId = `${renderSession.componentPath}.hook${currentHookIndex}`;
  currentHookIndex += 1;
  renderSession.didSeeHookId[hookId] = true;

  const guiState = guiStateForScriptObjectIdAndGuiKey(renderSession.scriptObjectId, renderSession.guiKey);
  let stateOfHookId = guiState.stateOfHookId;
  if (stateOfHookId === undefined) {
    stateOfHookId = createStateOfHookId();
    guiState.stateOfHookId = stateOfHookId;
  }

  if (!(hookId in stateOfHookId)) {
    stateOfHookId[hookId] =
      typeof initialState === "function"
        ? (initialState as () => T)()
        : initialState;
  }

  const setState = (nextState: NextStateValue<T>) => {
    const previousState = stateOfHookId[hookId] as T;
    stateOfHookId[hookId] =
      typeof nextState === "function"
        ? (nextState as (prevState: T) => T)(previousState)
        : nextState;
    renderGui(renderSession.scriptObjectId, renderSession.guiKey);
  };

  return [stateOfHookId[hookId] as T, setState];
};
