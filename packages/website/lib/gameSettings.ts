import { revalidatePath } from "next/cache";
import { eq } from "drizzle-orm";
import { db } from "@/lib/db";
import { getGameIfOwned } from "@/lib/getGameIfOwned";
import { createGameRow } from "@/app/server-actions/createGameRow";
import { hasProfanity } from "@/lib/siteProfanityFilter";
import {
  games,
  gameWorlds,
  runningServers,
  startingServers,
  userLikes,
  userProfiles,
} from "@/supabase/migrations/schema";

export type NewGameSettings = {
  name: string;
  description: string;
  suggestedMinPlayers: number;
  maxPlayers: number;
  isPublished: boolean;
  initialMessage: string | null;
};

export type GameSettingsUpdate = {
  gameId: number;
  name: string;
  description: string;
  suggestedMinPlayers: number;
  maxPlayers: number;
  isPublished: boolean;
};

export type SavedGameSettings = {
  name: string;
  description: string | null;
  suggestedMinPlayers: number;
  maxPlayers: number;
  isPublished: boolean;
};

// TODO: AI-PICKED-VALUE: Titles stop at 200 characters. The form sets no limit, so this is only a stop on direct server action calls.
const MAX_GAME_NAME_LENGTH = 200;

// A game's title and description are held to the same profanity rules a username is. This runs on
// the server on the way in, so it catches an edit made straight through a server action as well as
// one made in the form, and the word list never reaches the page.
const checkForProfanity = ({
  name,
  description,
}: {
  name: string;
  description: string | null;
}) => {
  if (hasProfanity(name)) return "That game title is not allowed.";
  if (description !== null && hasProfanity(description)) return "That description is not allowed.";
  return null;
};


const revalidateGameSettingsPaths = ({
  gameId,
  creatorUserId,
}: {
  gameId: number;
  creatorUserId: number;
}) => {
  revalidatePath(`/games/${gameId}`);
  revalidatePath(`/profile/${creatorUserId}`);
  revalidatePath("/");
  revalidatePath("/create");
};

// Making a game clears these pages afterwards, and only knows who the creator is by their id, so
// the public id the profile page is keyed by is looked up here.
const revalidatePathsAfterCreatingGame = async ({
  gameId,
  creatorId,
}: {
  gameId: number;
  creatorId: string;
}) => {
  const [creator] = await db
    .select({ userId: userProfiles.userId })
    .from(userProfiles)
    .where(eq(userProfiles.id, creatorId))
    .limit(1);

  if (!creator) {
    throw new Error("Creator profile not found");
  }

  revalidateGameSettingsPaths({ gameId, creatorUserId: creator.userId });
};

export const createGameForCreator = async ({
  creatorId,
  name,
  description,
  suggestedMinPlayers,
  maxPlayers,
  isPublished,
  initialMessage,
}: NewGameSettings & {
  creatorId: string;
}): Promise<{ gameId: number }> => {
  const trimmedName = name.trim();
  const trimmedDescription = description.trim();
  if (!trimmedName) {
    throw new Error("Game name is required.");
  }
  if (trimmedName.length > MAX_GAME_NAME_LENGTH) {
    throw new Error(`Game name can be at most ${MAX_GAME_NAME_LENGTH} characters.`);
  }
  if (!Number.isInteger(maxPlayers) || maxPlayers < 1) {
    throw new Error("Max players must be a positive whole number.");
  }
  if (!Number.isInteger(suggestedMinPlayers) || suggestedMinPlayers < 1 || suggestedMinPlayers > maxPlayers) {
    throw new Error("Suggested min players must be a whole number from 1 to max players.");
  }
  const profanityProblem = checkForProfanity({
    name: trimmedName,
    description: trimmedDescription || null,
  });
  if (profanityProblem !== null) {
    throw new Error(profanityProblem);
  }

  const gameId = await createGameRow({
    creatorId,
    name: trimmedName,
    description: trimmedDescription || null,
    suggestedMinPlayers,
    maxPlayers,
    isPublished,
    initialMessage,
  });

  await revalidatePathsAfterCreatingGame({ gameId, creatorId });

  return { gameId };
};

export const getGameSettingsForCreator = async ({
  gameId,
  creatorId,
}: {
  gameId: number;
  creatorId: string;
}): Promise<SavedGameSettings | { error: string }> => {
  const game = await getGameIfOwned({ gameId, creatorId });
  if ("error" in game) {
    return game;
  }

  return {
    name: game.name,
    description: game.description,
    suggestedMinPlayers: game.suggestedMinPlayers,
    maxPlayers: game.maxPlayers,
    isPublished: game.isPublished ?? false,
  };
};

export const updateGameSettingsForCreator = async ({
  gameId,
  creatorId,
  name,
  description,
  suggestedMinPlayers,
  maxPlayers,
  isPublished,
}: GameSettingsUpdate & {
  creatorId: string;
}): Promise<SavedGameSettings | { error: string }> => {
  const game = await getGameIfOwned({ gameId, creatorId });
  if ("error" in game) {
    return game;
  }
  const trimmedName = name.trim();
  const trimmedDescription = description.trim();
  if (!trimmedName) {
    throw new Error("Game name is required.");
  }
  if (trimmedName.length > MAX_GAME_NAME_LENGTH) {
    throw new Error(`Game name can be at most ${MAX_GAME_NAME_LENGTH} characters.`);
  }
  if (!Number.isInteger(maxPlayers) || maxPlayers < 1) {
    throw new Error("Max players must be a positive whole number.");
  }
  if (!Number.isInteger(suggestedMinPlayers) || suggestedMinPlayers < 1 || suggestedMinPlayers > maxPlayers) {
    throw new Error("Suggested min players must be a whole number from 1 to max players.");
  }
  const profanityProblem = checkForProfanity({
    name: trimmedName,
    description: trimmedDescription || null,
  });
  if (profanityProblem !== null) {
    throw new Error(profanityProblem);
  }

  const [savedGame] = await db
    .update(games)
    .set({
      name: trimmedName,
      description: trimmedDescription || null,
      suggestedMinPlayers,
      maxPlayers,
      isPublished,
    })
    .where(eq(games.gameId, gameId))
    .returning({
      name: games.name,
      description: games.description,
      suggestedMinPlayers: games.suggestedMinPlayers,
      maxPlayers: games.maxPlayers,
      isPublished: games.isPublished,
    });

  if (!savedGame) {
    throw new Error("Game not found");
  }

  revalidateGameSettingsPaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return {
    name: savedGame.name,
    description: savedGame.description,
    suggestedMinPlayers: savedGame.suggestedMinPlayers,
    maxPlayers: savedGame.maxPlayers,
    isPublished: savedGame.isPublished ?? false,
  };
};

export const deleteGameForCreator = async ({
  gameId,
  creatorId,
}: {
  gameId: number;
  creatorId: string;
}) => {
  const game = await getGameIfOwned({ gameId, creatorId });
  if ("error" in game) {
    return game;
  }

  await db.transaction(async (tx) => {
    // Delete rows that point at the game before deleting the game and its saved worlds.
    await tx.delete(startingServers).where(eq(startingServers.gameId, gameId));
    await tx.delete(runningServers).where(eq(runningServers.gameId, gameId));
    await tx.delete(userLikes).where(eq(userLikes.gameId, gameId));
    await tx.delete(games).where(eq(games.gameId, gameId));
    await tx.delete(gameWorlds).where(eq(gameWorlds.gameId, gameId));
  });

  revalidateGameSettingsPaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return {
    profilePath: `/profile/${game.creatorUserId}`,
  };
};
