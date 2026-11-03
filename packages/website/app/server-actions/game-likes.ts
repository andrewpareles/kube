"use server";

import { revalidatePath } from "next/cache";
import { and, eq } from "drizzle-orm";
import { db } from "@/lib/db";
import { requireUser } from "@/lib/siteUser";
import { games, userFavorites, userLikes, userProfiles } from "@/supabase/migrations/schema";

type GameLikeState = {
  isLikedByViewer: boolean;
};

type GameFavoriteState = {
  isFavoritedByViewer: boolean;
};

const getProfileId = async () => (await requireUser()).id;

const getLikeableGame = async ({
  gameId,
  profileId,
}: {
  gameId: number;
  profileId: string;
}) => {
  const [game] = await db
    .select({
      creatorId: games.creatorId,
      creatorUserId: userProfiles.userId,
      isPublished: games.isPublished,
    })
    .from(games)
    .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
    .where(eq(games.gameId, gameId))
    .limit(1);

  if (!game) {
    throw new Error("Game not found");
  }
  if (!game.isPublished && game.creatorId !== profileId) {
    throw new Error("Game is not published");
  }

  return game;
};

const getGameLikeState = async ({
  gameId,
  profileId,
}: {
  gameId: number;
  profileId: string;
}): Promise<GameLikeState> => {
  const [viewerLike] = await db
    .select({
      profileId: userLikes.profileId,
    })
    .from(userLikes)
    .where(
      and(
        eq(userLikes.gameId, gameId),
        eq(userLikes.profileId, profileId),
      ),
    )
    .limit(1);

  return {
    isLikedByViewer: Boolean(viewerLike),
  };
};

const revalidateGameLikePaths = ({
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

export const likeGame = async (gameId: number): Promise<GameLikeState> => {
  const profileId = await getProfileId();
  const game = await getLikeableGame({ gameId, profileId });

  await db
    .insert(userLikes)
    .values({
      gameId,
      profileId,
    })
    .onConflictDoNothing();

  revalidateGameLikePaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return getGameLikeState({ gameId, profileId });
};

export const unlikeGame = async (gameId: number): Promise<GameLikeState> => {
  const profileId = await getProfileId();
  const game = await getLikeableGame({ gameId, profileId });

  await db
    .delete(userLikes)
    .where(
      and(
        eq(userLikes.gameId, gameId),
        eq(userLikes.profileId, profileId),
      ),
    );

  revalidateGameLikePaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return getGameLikeState({ gameId, profileId });
};

const getGameFavoriteState = async ({
  gameId,
  profileId,
}: {
  gameId: number;
  profileId: string;
}): Promise<GameFavoriteState> => {
  const [viewerFavorite] = await db
    .select({
      profileId: userFavorites.profileId,
    })
    .from(userFavorites)
    .where(
      and(
        eq(userFavorites.gameId, gameId),
        eq(userFavorites.profileId, profileId),
      ),
    )
    .limit(1);

  return {
    isFavoritedByViewer: Boolean(viewerFavorite),
  };
};

export const favoriteGame = async (gameId: number): Promise<GameFavoriteState> => {
  const profileId = await getProfileId();
  await getLikeableGame({ gameId, profileId });

  await db
    .insert(userFavorites)
    .values({
      gameId,
      profileId,
    })
    .onConflictDoNothing();

  revalidatePath(`/games/${gameId}`);

  return getGameFavoriteState({ gameId, profileId });
};

export const unfavoriteGame = async (gameId: number): Promise<GameFavoriteState> => {
  const profileId = await getProfileId();
  await getLikeableGame({ gameId, profileId });

  await db
    .delete(userFavorites)
    .where(
      and(
        eq(userFavorites.gameId, gameId),
        eq(userFavorites.profileId, profileId),
      ),
    );

  revalidatePath(`/games/${gameId}`);

  return getGameFavoriteState({ gameId, profileId });
};
