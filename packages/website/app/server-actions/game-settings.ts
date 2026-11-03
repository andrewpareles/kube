"use server";

import {
  createGameForCreator,
  deleteGameForCreator,
  updateGameSettingsForCreator,
  type GameSettingsUpdate,
  type NewGameSettings,
  type SavedGameSettings,
} from "@/lib/gameSettings";
import { requireUser } from "@/lib/siteUser";
import { getCreatorIdIfEditable } from "@/lib/getGameIfOwned";

export type { GameSettingsUpdate, NewGameSettings, SavedGameSettings };

const getCurrentCreatorId = async () => (await requireUser()).id;

export const createGame = async (
  newGameSettings: NewGameSettings,
): Promise<{ gameId: number }> => {
  const creatorId = await getCurrentCreatorId();
  return createGameForCreator({ ...newGameSettings, creatorId });
};

export const updateGameSettings = async (
  gameSettingsUpdate: GameSettingsUpdate,
  inviteToken: string | null,
): Promise<SavedGameSettings | { error: string }> => {
  const editor = await getCreatorIdIfEditable(gameSettingsUpdate.gameId, inviteToken);
  if ("error" in editor) {
    return editor;
  }
  return updateGameSettingsForCreator({
    ...gameSettingsUpdate,
    creatorId: editor.creatorId,
  });
};

export const deleteGame = async (gameId: number) => {
  const creatorId = await getCurrentCreatorId();
  return deleteGameForCreator({ gameId, creatorId });
};
