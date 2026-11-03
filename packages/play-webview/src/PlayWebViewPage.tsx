import { useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import { GameChat } from "./GameChat";
import { getPageState, postToHost, subscribeToPageState, type GameChat as GameChatState, type PlayersListEntry } from "./hostBridge";
import { MAIN_MENU_FADE_MS, MainMenu } from "./MainMenu";
import { PlayersList } from "./PlayersList";
import { readSavedGraphicsQuality, readSavedSetting, readSavedVolume, saveSetting } from "./savedSettings";
import { ScriptGuiRoots } from "./ScriptGuiRoots";
import { SplashScreen } from "./SplashScreen";

const CircleButton = ({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) => (
  <button
    type="button"
    aria-label={label}
    onClick={onClick}
    className="flex size-10 items-center justify-center rounded-full bg-neutral-900/75 text-white shadow-lg hover:bg-neutral-800/90"
  >
    {children}
  </button>
);

// TODO: AI-PICKED-VALUE: 40-point circle buttons, drawn with the chat and the menu at z-index 1000 so script GUIs stay under them.
const GameControls = ({ gameChat, playersList }: { gameChat: GameChatState; playersList: PlayersListEntry[] }) => {
  const [isChatOpen, setIsChatOpen] = useState(() => readSavedSetting("isChatOpen") !== "false");
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [isMenuShown, setIsMenuShown] = useState(false);
  const [volume, setVolume] = useState(readSavedVolume);
  const [graphicsQuality, setGraphicsQuality] = useState(readSavedGraphicsQuality);
  const [isPlayersListShown, setIsPlayersListShown] = useState(false);

  useEffect(() => {
    saveSetting("isChatOpen", String(isChatOpen));
  }, [isChatOpen]);

  // The game starts at full volume, so the saved volume goes to the host as soon as the game shows.
  useEffect(() => {
    saveSetting("volume", String(volume));
    postToHost({ setVolume: volume });
  }, [volume]);

  useEffect(() => {
    saveSetting("graphicsQuality", graphicsQuality);
    postToHost({ setGraphicsQuality: graphicsQuality });
  }, [graphicsQuality]);

  // The host lets go of a locked mouse while the menu is open, so its buttons can be clicked.
  useEffect(() => {
    postToHost({ setIsMenuOpen: isMenuOpen });
  }, [isMenuOpen]);

  useEffect(() => {
    if (isMenuOpen) {
      setIsMenuShown(true);
      return;
    }
    const timeout = setTimeout(() => setIsMenuShown(false), MAIN_MENU_FADE_MS);
    return () => clearTimeout(timeout);
  }, [isMenuOpen]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.repeat || (event.target instanceof HTMLInputElement && event.target.type === "text")) {
        return;
      }
      setIsMenuOpen((isMenuOpen) => !isMenuOpen);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Tab" && !(event.target instanceof HTMLInputElement && event.target.type === "text")) {
        setIsPlayersListShown(true);
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === "Tab") {
        setIsPlayersListShown(false);
      }
    };
    const onBlur = () => setIsPlayersListShown(false);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  return (
    <>
      <div className="pointer-events-none absolute top-3 left-3 z-[1000] flex flex-col items-start gap-2 *:pointer-events-auto">
        <div className="flex gap-2">
          <CircleButton label="Menu" onClick={() => setIsMenuOpen(true)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" className="size-5">
              <path d="M4 6h16M4 12h16M4 18h16" />
            </svg>
          </CircleButton>
          <CircleButton label="Chat" onClick={() => setIsChatOpen(!isChatOpen)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinejoin="round" className="size-5">
              <path d="M8 20.2A9 9 0 1 0 3.8 16L2.5 21.5Z" />
            </svg>
          </CircleButton>
        </div>
        <GameChat gameChat={gameChat} isOpen={isChatOpen} onOpen={() => setIsChatOpen(true)} />
      </div>
      {(isMenuOpen || isMenuShown) && (
        <MainMenu
          isOpen={isMenuOpen}
          volume={volume}
          onVolumeChange={setVolume}
          graphicsQuality={graphicsQuality}
          onGraphicsQualityChange={setGraphicsQuality}
          onLeave={() => postToHost("leaveGame")}
          onClose={() => setIsMenuOpen(false)}
        />
      )}
      {isPlayersListShown && <PlayersList playersList={playersList} />}
    </>
  );
};

// The whole page: the splash screen, or the game's GUI and chat on top of the game.
export const PlayWebViewPage = () => {
  const pageState = useSyncExternalStore(subscribeToPageState, getPageState);
  if (pageState === null) {
    return null;
  }
  if ("splash" in pageState) {
    return <SplashScreen splashScreenContent={pageState.splash} />;
  }
  return (
    <>
      <ScriptGuiRoots scriptGuiRoots={pageState.play.scriptGuiRoots} />
      {pageState.play.gameChat !== null && <GameControls gameChat={pageState.play.gameChat} playersList={pageState.play.playersList} />}
    </>
  );
};
