import "server-only";

import { eq } from "drizzle-orm";
import { db } from "@/lib/db";
import { getEditorAccess } from "@/lib/editorAccess";
import { getUser } from "@/lib/siteUser";
import { games, userProfiles } from "@/supabase/migrations/schema";

// The one owner check behind every website-level change to a game, so that publishing a game and
// renaming it cannot drift into two different ideas of whose game it is.
// A refusal is returned rather than thrown, since production hides a thrown error's message from the page.
export const getGameIfOwned = async ({
  gameId,
  creatorId,
}: {
  gameId: number;
  creatorId: string;
}) => {
  const [game] = await db
    .select({
      name: games.name,
      description: games.description,
      creatorId: games.creatorId,
      creatorUserId: userProfiles.userId,
      suggestedMinPlayers: games.suggestedMinPlayers,
      maxPlayers: games.maxPlayers,
      isPublished: games.isPublished,
    })
    .from(games)
    .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
    .where(eq(games.gameId, gameId))
    .limit(1);

  if (!game) {
    return { error: "This game no longer exists." };
  }
  if (game.creatorId !== creatorId) {
    return { error: "Only this game's creator can do that." };
  }

  return game;
};

// Anyone who can open the editor can change the game's settings and visibility, made as its creator like everything else there.
export const getCreatorIdIfEditable = async (
  gameId: number,
  inviteToken: string | null,
): Promise<{ creatorId: string } | { error: string }> => {
  const user = await getUser();
  const game = await getEditorAccess(gameId, user?.id ?? null, inviteToken);
  if (!game) {
    return { error: "You do not have access to this editor. The invite link may have been revoked." };
  }
  return { creatorId: game.creatorId };
};
