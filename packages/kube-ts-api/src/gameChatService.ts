import { ourGlobals } from "./object_types/__globalOps__";
import { removeFnFromArray } from "./lib/removeFnFromArray";
import type { UserId } from "./object_types/shared";

/** Field names match the GameChatMessage struct the server sends. */
export type GameChatMessage = {
  time_live_server_ms: number;
  userid: UserId;
  username: string;
  message: string;
};

export type GameChatCallback = (gameChatMessage: GameChatMessage) => void;

/**
 * - This lets you use a script to listen for chats sent in the game.
 * - Only callable by a Server Script.
 */
export type GameChatService = {
  addGameChatListener: (callback: GameChatCallback) => void;
  removeGameChatListener: (callback: GameChatCallback) => void;
};

export const gameChatService: GameChatService = {
  addGameChatListener: (callback: GameChatCallback) => {
    ourGlobals.__onGameChatMessageCallbacks__.push(callback);
  },
  removeGameChatListener: (callback: GameChatCallback) => {
    removeFnFromArray(ourGlobals.__onGameChatMessageCallbacks__, callback);
  },
};
