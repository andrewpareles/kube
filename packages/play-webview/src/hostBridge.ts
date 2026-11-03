import type { MouseButtonName } from "../../kube-ts-api/src/clientInputService";
import type { GuiRenderedRoot } from "../../kube-ts-api/src/gui/guiTypes";

// The desktop client (the host) and this page talk in JSON. The host calls `receiveFromHost` with a
// HostToWebViewMessage, and the page answers through `ipc.postMessage`, which wry adds to the page.
// These types match the ones in crates/client/src/play_web_view.rs.

export type SplashScreenContent = {
  titleText: string;
  subText: string | null;
  thumbnailUrl: string | null;
  isLoading: boolean;
};

export type ScriptGuiRoot = {
  scriptObjectId: number;
  guiKey: string;
  root: GuiRenderedRoot;
};

export type GameChatMessage = {
  time_live_server_ms: number;
  userid: number;
  username: string;
  message: string;
};

export type GameChatEntry =
  | { Message: GameChatMessage }
  | { PlayerJoined: { username: string } }
  | { PlayerLeft: { username: string } };

export type GraphicsQuality = "low" | "high";

export type PlayersListEntry = {
  username: string;
  isLocalPlayer: boolean;
  pingMs: number | null;
};

export type GameChat = {
  entries: GameChatEntry[];
  error: string | null;
  isDisconnected: boolean;
  maxMessageChars: number;
};

export type HostToWebViewMessage =
  | { showSplashScreen: SplashScreenContent }
  | "showPlayScreen"
  | { setScriptGuiRoots: ScriptGuiRoot[] }
  | { setGameChat: GameChat }
  | { setPlayersList: PlayersListEntry[] };

// A rectangle of the page in physical pixels, the units the host measures the window in.
export type UiRect = { x: number; y: number; width: number; height: number };

export type WebViewToHostMessage =
  | "ready"
  | { setUiRects: { uiOnlyRects: UiRect[]; scrollableRects: UiRect[] } }
  | { setIsChatFocused: boolean }
  | { setIsMenuOpen: boolean }
  | { guiHandler: { scriptObjectId: number; guiKey: string; handlerId: string; button: MouseButtonName | null } }
  | { sendGameChatMessage: { message: string } }
  | { setVolume: number }
  | { setGraphicsQuality: GraphicsQuality }
  | "leaveGame";

export type PageState =
  | { splash: SplashScreenContent }
  | { play: { scriptGuiRoots: ScriptGuiRoot[]; gameChat: GameChat | null; playersList: PlayersListEntry[] } }
  | null;

declare global {
  var receiveFromHost: (message: HostToWebViewMessage) => void;
  var ipc: { postMessage: (message: string) => void } | undefined;
}

export const postToHost = (message: WebViewToHostMessage) => {
  globalThis.ipc?.postMessage(JSON.stringify(message));
};

// -------------------------- page state -------------------------------

// The page shows whatever the host last sent, so the state lives here and React reads it with useSyncExternalStore.
let pageState: PageState = null;
const pageStateListeners = new Set<() => void>();

const setPageState = (nextPageState: PageState) => {
  pageState = nextPageState;
  for (const pageStateListener of pageStateListeners) {
    pageStateListener();
  }
};

export const getPageState = () => pageState;

export const subscribeToPageState = (pageStateListener: () => void) => {
  pageStateListeners.add(pageStateListener);
  return () => {
    pageStateListeners.delete(pageStateListener);
  };
};

// WebKit starts a transition on the frame after its class changes and draws that frame at the transition's start. The
// host's messages land between frames, so a meter a script sets every tick would restart its transition every frame and
// never move. A change made in requestAnimationFrame starts moving on the frame that draws it, so new GUI roots wait for it.
let scriptGuiRootsForNextFrame: ScriptGuiRoot[] | null = null;

const setScriptGuiRootsOnNextFrame = (scriptGuiRoots: ScriptGuiRoot[]) => {
  if (scriptGuiRootsForNextFrame === null) {
    requestAnimationFrame(() => {
      if (scriptGuiRootsForNextFrame !== null && pageState !== null && "play" in pageState) {
        setPageState({ play: { ...pageState.play, scriptGuiRoots: scriptGuiRootsForNextFrame } });
      }
      scriptGuiRootsForNextFrame = null;
    });
  }
  scriptGuiRootsForNextFrame = scriptGuiRoots;
};

// GUI roots and chat only belong to the play screen, so they are dropped if a splash screen is showing.
globalThis.receiveFromHost = (message: HostToWebViewMessage) => {
  if (message === "showPlayScreen") {
    setPageState({ play: { scriptGuiRoots: [], gameChat: null, playersList: [] } });
    return;
  }
  if ("showSplashScreen" in message) {
    setPageState({ splash: message.showSplashScreen });
    return;
  }
  if (pageState === null || !("play" in pageState)) {
    return;
  }
  if ("setScriptGuiRoots" in message) {
    setScriptGuiRootsOnNextFrame(message.setScriptGuiRoots);
    return;
  }
  if ("setPlayersList" in message) {
    setPageState({ play: { ...pageState.play, playersList: message.setPlayersList } });
    return;
  }
  setPageState({ play: { ...pageState.play, gameChat: message.setGameChat } });
};
