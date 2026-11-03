import type { PlayersListEntry } from "./hostBridge";

// TODO: AI-PICKED-VALUE: A 320-point-wide list in the middle of the screen, no taller than 80% of it.
export const PlayersList = ({ playersList }: { playersList: PlayersListEntry[] }) => {
  const localPlayerFirstList = [...playersList.filter((entry) => entry.isLocalPlayer), ...playersList.filter((entry) => !entry.isLocalPlayer)];
  return (
    <div className="pointer-events-none absolute inset-0 z-[1000] flex items-center justify-center">
      <div className="flex max-h-[80vh] w-80 flex-col gap-1 overflow-hidden rounded-md bg-neutral-900/80 p-3 text-sm text-white shadow-lg">
        {localPlayerFirstList.map((playersListEntry, playerIndex) => (
          <div key={playerIndex} className={`flex justify-between gap-2 px-2 py-1 ${playersListEntry.isLocalPlayer ? "text-sky-200" : ""}`}>
            <span className="truncate">{playersListEntry.username}</span>
            {playersListEntry.pingMs !== null && <span className="shrink-0 text-neutral-400">{playersListEntry.pingMs} ms</span>}
          </div>
        ))}
      </div>
    </div>
  );
};
