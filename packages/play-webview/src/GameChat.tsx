import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { postToHost, type GameChat as GameChatState, type GameChatEntry } from "./hostBridge";

// GameChat is the chat between players in a running game. Only messages the server echoes back show
// up here, and the host does the sending, so this only draws the history and hands over what was typed.

const GameChatEntryView = ({ entry }: { entry: GameChatEntry }) => {
  if ("Message" in entry) {
    return (
      <p className="break-words">
        <span className="text-sky-400">{entry.Message.username}: </span>
        {entry.Message.message}
      </p>
    );
  }
  if ("PlayerJoined" in entry) {
    return <p className="text-neutral-400">{entry.PlayerJoined.username} joined</p>;
  }
  return <p className="text-neutral-400">{entry.PlayerLeft.username} left</p>;
};

// TODO: AI-PICKED-VALUE: Use a 320-point-wide panel, a 200-point message list, and a 12-point corner inset.
export const GameChat = ({ gameChat, isOpen, onOpen }: { gameChat: GameChatState; isOpen: boolean; onOpen: () => void }) => {
  const { entries, error, isDisconnected, maxMessageChars } = gameChat;
  const [draftMessage, setDraftMessage] = useState("");
  const [draftError, setDraftError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const historyRef = useRef<HTMLDivElement>(null);
  const isHistoryAtBottomRef = useRef(true);
  const shownError = isDisconnected ? "Chat disconnected." : (draftError ?? error);

  // Pressing / anywhere starts typing, and the slash itself is not typed.
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "/" || (event.target instanceof HTMLInputElement && event.target.type === "text")) {
        return;
      }
      event.preventDefault();
      flushSync(onOpen);
      inputRef.current?.focus();
    };
    document.addEventListener("keydown", onKeyDown, { capture: true });
    return () => document.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [onOpen]);

  // New messages keep the history scrolled to the bottom, unless someone scrolled up to read.
  useLayoutEffect(() => {
    const history = historyRef.current;
    if (history !== null && isHistoryAtBottomRef.current) {
      history.scrollTop = history.scrollHeight;
    }
  }, [entries, isOpen]);

  // Enter sends and returns to the game. A message that cannot be sent stays in the box to fix.
  const onInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.currentTarget.blur();
      return;
    }
    if (event.key !== "Enter") {
      return;
    }
    if (isDisconnected) {
      return;
    }
    const message = draftMessage.trim();
    if (/\p{Cc}/u.test(message)) {
      setDraftError("Chat messages cannot contain control characters.");
      return;
    }
    if (message !== "") {
      postToHost({ sendGameChatMessage: { message } });
      setDraftMessage("");
      setDraftError(null);
    }
    event.currentTarget.blur();
  };

  return (
    <div
      data-ui-only=""
      className={`${isOpen ? "flex" : "hidden"} w-80 max-w-[calc(100vw-40px)] flex-col gap-2 rounded-md bg-neutral-900/75 p-3 text-sm text-white shadow-lg`}
    >
      <div
        ref={historyRef}
        onScroll={(event) => {
          const history = event.currentTarget;
          isHistoryAtBottomRef.current = history.scrollTop + history.clientHeight >= history.scrollHeight - 1;
        }}
        className="flex h-[200px] max-h-[calc(100vh-100px)] flex-col gap-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {entries.map((entry, entryIndex) => (
          <GameChatEntryView key={entryIndex} entry={entry} />
        ))}
      </div>
      <input
        ref={inputRef}
        value={draftMessage}
        // The server counts characters rather than UTF-16 units, so an emoji is one character here too.
        onChange={(event) => setDraftMessage(Array.from(event.currentTarget.value).slice(0, maxMessageChars).join(""))}
        onKeyDown={onInputKeyDown}
        // The host reads keys straight from the OS, so it needs to know when they are typing instead of game input.
        onFocus={() => postToHost({ setIsChatFocused: true })}
        onBlur={() => postToHost({ setIsChatFocused: false })}
        placeholder="Press / to chat"
        className="rounded-md bg-white/10 px-2.5 py-1.5 outline-none placeholder:text-neutral-400 focus:bg-white/15 focus:ring-1 focus:ring-sky-400"
      />
      {shownError !== null && <p className="text-neutral-400">{shownError}</p>}
    </div>
  );
};
