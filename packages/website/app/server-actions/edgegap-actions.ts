"use server";
// TODO whenever we create something on Hathora, always save its id to supabase
// TODO (not here) in rust server, if player joins a room, kick them from all other rooms they are in
// TODO in future matchmaking based on region https://hathora.dev/docs/gaming/guides/ping-service; need a good amount of users first.

import { headers } from "next/headers";
import { revalidatePath } from "next/cache";
import { db } from "@/lib/db";
import { getCreatorIdIfEditable, getGameIfOwned } from "@/lib/getGameIfOwned";
import { requireUser } from "@/lib/siteUser";
import { gameThumbnailUuid } from "@/lib/siteGames";
import { createDesktopAuth } from "@/lib/desktopAuth";
import {
  EDGEGAP_DEPLOY_VERSION_NAME,
  EDGEGAP_GAME_ID_ENV_KEY,
  EDGEGAP_GAMEWORLD_VERSION_NUM_ENV_KEY,
  type EdgegapDeploymentCreateResponse,
  getEdgegapAuthorizationHeader,
  getEdgegapDeploymentTags,
  getEdgegapWebhookUrl,
  getRequiredEnvVar,
  getSecretEnvVars,
} from "@/lib/edgegapApi";
import {
  getFullestServerWithRoom,
  isSpecificRunningServerAvailableToPlayer,
} from "@/lib/playServerFlow";
import {
  gameWorlds,
  games,
  userDesktopPlayAuth,
  runningPlayerSessions,
  runningServers,
  startingPlayerSessions,
  startingServers,
  userProfiles,
} from "@/supabase/migrations/schema";
import { and, count, desc, eq, sql, type SQL } from "drizzle-orm";

type PlayLaunchInfo = {
  requestId: string;
  playAuth: string;
  thumbnailUuid: string | null;
};

// The play link carries the game's thumbnail so the app can show it while the game loads.
const getThumbnailUuid = async (gameId: number) => {
  const [game] = await db.select({ thumbnailUuid: gameThumbnailUuid }).from(games).where(eq(games.gameId, gameId));
  return game?.thumbnailUuid ?? null;
};

const getRequesterIpAddress = async () => {
  const requestHeaders = await headers();
  const forwardedFor = requestHeaders.get("x-forwarded-for");
  if (forwardedFor) {
    return forwardedFor.split(",")[0].trim();
  }

  const realIp = requestHeaders.get("x-real-ip");
  if (realIp) {
    return realIp.trim();
  }

  return "127.0.0.1";
};
const fetchEdgegapJson = async <TJson>({
  url,
  method,
  body,
}: {
  url: string;
  method: "GET" | "POST";
  body: unknown | undefined;
}): Promise<TJson> => {
  const response = await fetch(url, {
    method,
    headers: {
      Authorization: getEdgegapAuthorizationHeader(),
      "Content-Type": "application/json",
    },
    body: body ? JSON.stringify(body) : undefined,
    cache: "no-store",
  });

  if (!response.ok) {
    const responseBody = await response.text();
    throw new Error(
      `Edgegap request failed (${response.status} ${response.statusText}): ${responseBody}`,
    );
  }

  const responseJson: TJson = await response.json();
  return responseJson;
};

// A running_player_sessions row is written the moment Play is pressed, before the desktop app has
// done anything, because the game server needs the row to already exist to let that client in. When
// the app never launches - not installed, deeplink dismissed, launch failed - that row is left behind
// holding a seat nobody is sitting in.
//
// A reservation is therefore only good for a short while. A player who has not reached the game by
// then is treated as not having joined: the row goes, the seat goes back, and if their app does open
// later the game server turns it away and tells them to launch from the website again.
//
// This runs here rather than on a schedule because it only has to be true at the moment it is read.
// It is the first thing a launch does after taking the game lock, so the capacity counted just below
// never includes a reservation that has run out, and a launch nobody is competing with never pays
// for cleanup it does not need.
const RESERVATION_GRACE_SECONDS = 30;

type DbTransaction = Parameters<Parameters<typeof db.transaction>[0]>[0];

const deleteExpiredReservationsOfRunningServers = async ({
  tx,
  runningServerFilter,
}: {
  tx: DbTransaction;
  runningServerFilter: SQL;
}) => {
  await tx.execute(sql`
    delete from running_player_sessions
    where connected_at is null
      and reserved_at < now() - make_interval(secs => ${RESERVATION_GRACE_SECONDS})
      and request_id in (
        select request_id
        from running_servers
        where ${runningServerFilter}
      )
  `);
};

// ---------------------------------- creators ----------------------------------


const revalidateGamePublishStatePaths = ({
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

export const publishGame = async (gameId: number, inviteToken: string | null) => {
  const editor = await getCreatorIdIfEditable(gameId, inviteToken);
  if ("error" in editor) {
    return editor;
  }
  const game = await getGameIfOwned({ gameId, creatorId: editor.creatorId });
  if ("error" in game) {
    return game;
  }

  await db
    .update(games)
    .set({
      isPublished: true,
    })
    .where(eq(games.gameId, gameId));

  revalidateGamePublishStatePaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return { isPublished: true };
};

export const unpublishGame = async (gameId: number, inviteToken: string | null) => {
  const editor = await getCreatorIdIfEditable(gameId, inviteToken);
  if ("error" in editor) {
    return editor;
  }
  const game = await getGameIfOwned({ gameId, creatorId: editor.creatorId });
  if ("error" in game) {
    return game;
  }

  await db
    .update(games)
    .set({
      isPublished: false,
    })
    .where(eq(games.gameId, gameId));

  revalidateGamePublishStatePaths({
    gameId,
    creatorUserId: game.creatorUserId,
  });

  return { isPublished: false };
};

// ---------------------------------- players ----------------------------------

export const getJoinGameInfoAnyGame = async (
  gameId: number,
): Promise<PlayLaunchInfo> => {
  const user = await requireUser();

  const requesterIpAddress = await getRequesterIpAddress();
  const { authStr: playAuth, hash: playAuthHash } = createDesktopAuth();
  const requestId = await db.transaction(async (tx) => {
    const savePlayAuth = async (reservedRequestId: string) => {
      await tx
        .update(games)
        .set({
          totalVisits: sql`${games.totalVisits} + 1`,
        })
        .where(eq(games.gameId, gameId));
      await tx
        .insert(userDesktopPlayAuth)
        .values({
          profileId: user.id,
          playAuthHash,
        })
        .onConflictDoUpdate({
          target: userDesktopPlayAuth.profileId,
          set: {
            playAuthHash,
          },
        });
      return reservedRequestId;
    };

    await tx.execute(sql`select 1 from user_profiles where id = ${user.id} for update`);
    await tx.execute(
      sql`select 1 from game_metadata where game_id = ${gameId} for update`,
    );

    const [game] = await tx
      .select({
        isPublished: games.isPublished,
        maxPlayers: games.maxPlayers,
      })
      .from(games)
      .where(eq(games.gameId, gameId))
      .limit(1);

    if (!game) {
      throw new Error("Game not found");
    }
    if (!game.isPublished) {
      throw new Error("Game is not published");
    }
    const [latestGameworld] = await tx
      .select({ versionNum: gameWorlds.versionNum })
      .from(gameWorlds)
      .where(eq(gameWorlds.gameId, gameId))
      .orderBy(desc(gameWorlds.versionNum))
      .limit(1);
    if (!latestGameworld) {
      throw new Error("Error finding gameworld for this game");
    }

    await tx
      .delete(startingPlayerSessions)
      .where(eq(startingPlayerSessions.profileId, user.id));
    await tx
      .delete(runningPlayerSessions)
      .where(eq(runningPlayerSessions.profileId, user.id));

    await deleteExpiredReservationsOfRunningServers({
      tx,
      runningServerFilter: sql`game_id = ${gameId}`,
    });

    // Fill the fullest running server first so new launches pack into existing capacity.
    const runningServerRows = await tx
      .select({
        requestId: runningServers.requestId,
        maxPlayers: runningServers.maxPlayers,
        playerCount: count(runningPlayerSessions.profileId),
      })
      .from(runningServers)
      .leftJoin(
        runningPlayerSessions,
        eq(runningPlayerSessions.requestId, runningServers.requestId),
      )
      .where(
        and(
          eq(runningServers.gameId, gameId),
          eq(runningServers.gameworldVersionNum, latestGameworld.versionNum),
        ),
      )
      .groupBy(runningServers.requestId, runningServers.maxPlayers);

    const runningServerWithRoom = getFullestServerWithRoom(runningServerRows);
    if (runningServerWithRoom) {
      await tx
        .insert(runningPlayerSessions)
        .values({
          profileId: user.id,
          requestId: runningServerWithRoom.requestId,
        });
      return savePlayAuth(runningServerWithRoom.requestId);
    }

    // If no running server has room, keep filling an in-flight starting server before creating another.
    const startingServerRows = await tx
      .select({
        requestId: startingServers.requestId,
        maxPlayers: startingServers.maxPlayers,
        playerCount: count(startingPlayerSessions.profileId),
      })
      .from(startingServers)
      .leftJoin(
        startingPlayerSessions,
        eq(startingPlayerSessions.requestId, startingServers.requestId),
      )
      .where(
        and(
          eq(startingServers.gameId, gameId),
          eq(startingServers.gameworldVersionNum, latestGameworld.versionNum),
        ),
      )
      .groupBy(startingServers.requestId, startingServers.maxPlayers);

    const startingServerWithRoom = getFullestServerWithRoom(startingServerRows);

    if (startingServerWithRoom) {
      await tx
        .insert(startingPlayerSessions)
        .values({
          profileId: user.id,
          requestId: startingServerWithRoom.requestId,
        });
      return savePlayAuth(startingServerWithRoom.requestId);
    }

    // If nothing reusable exists, create a fresh Edgegap deployment and start tracking it as starting.
    const deploymentResponse =
      await fetchEdgegapJson<EdgegapDeploymentCreateResponse>({
        url: "https://api.edgegap.com/v2/deployments",
        method: "POST",
        body: {
          application: getRequiredEnvVar("EDGEGAP_APPLICATION_NAME"),
          version: EDGEGAP_DEPLOY_VERSION_NAME,
          users: [
            {
              user_type: "ip_address",
              user_data: {
                ip_address: requesterIpAddress,
              },
            },
          ],
          webhook_on_ready: {
            url: getEdgegapWebhookUrl({ lifecycleEvent: "ready" }),
          },
          webhook_on_error: {
            url: getEdgegapWebhookUrl({ lifecycleEvent: "error" }),
          },
          webhook_on_terminated: {
            url: getEdgegapWebhookUrl({ lifecycleEvent: "terminated" }),
          },
          environment_variables: [
            {
              key: EDGEGAP_GAME_ID_ENV_KEY,
              value: gameId.toString(),
              is_hidden: false,
            },
            {
              key: EDGEGAP_GAMEWORLD_VERSION_NUM_ENV_KEY,
              value: latestGameworld.versionNum.toString(),
              is_hidden: false,
            },
            ...getSecretEnvVars("gameServer"),
          ],
          tags: getEdgegapDeploymentTags({
            gameId,
            gameworldVersionNum: latestGameworld.versionNum,
            maxPlayers: game.maxPlayers,
          }),
        },
      });

    const startingAt = new Date().toISOString();
    await tx
      .insert(startingServers)
      .values({
        requestId: deploymentResponse.request_id,
        gameId,
        maxPlayers: game.maxPlayers,
        gameworldVersionNum: latestGameworld.versionNum,
        startingAt,
      });
    await tx
      .insert(startingPlayerSessions)
      .values({
        profileId: user.id,
        requestId: deploymentResponse.request_id,
      });

    return savePlayAuth(deploymentResponse.request_id);
  });
  return {
    requestId,
    playAuth,
    thumbnailUuid: await getThumbnailUuid(gameId),
  };
};

export const getJoinGameInfoSpecificGame = async (
  gameId: number,
  requestId: string,
): Promise<PlayLaunchInfo> => {
  const user = await requireUser();

  const { authStr: playAuth, hash: playAuthHash } = createDesktopAuth();
  const reservedRequestId = await db.transaction(async (tx) => {
    const savePlayAuth = async (reservedRequestId: string) => {
      await tx
        .update(games)
        .set({
          totalVisits: sql`${games.totalVisits} + 1`,
        })
        .where(eq(games.gameId, gameId));
      await tx
        .insert(userDesktopPlayAuth)
        .values({
          profileId: user.id,
          playAuthHash,
        })
        .onConflictDoUpdate({
          target: userDesktopPlayAuth.profileId,
          set: {
            playAuthHash,
          },
        });
      return reservedRequestId;
    };

    await tx.execute(sql`select 1 from user_profiles where id = ${user.id} for update`);
    await tx.execute(
      sql`select 1 from game_metadata where game_id = ${gameId} for update`,
    );

    const [game] = await tx
      .select({
        isPublished: games.isPublished,
      })
      .from(games)
      .where(eq(games.gameId, gameId))
      .limit(1);

    if (!game) {
      throw new Error("Game not found");
    }
    if (!game.isPublished) {
      throw new Error("Game is not published");
    }
    const [latestGameworld] = await tx
      .select({ versionNum: gameWorlds.versionNum })
      .from(gameWorlds)
      .where(eq(gameWorlds.gameId, gameId))
      .orderBy(desc(gameWorlds.versionNum))
      .limit(1);
    if (!latestGameworld) {
      throw new Error("Error finding gameworld for this game");
    }

    const [runningServer] = await tx
      .select({
        requestId: runningServers.requestId,
        gameId: runningServers.gameId,
        maxPlayers: runningServers.maxPlayers,
        gameworldVersionNum: runningServers.gameworldVersionNum,
      })
      .from(runningServers)
      .where(eq(runningServers.requestId, requestId))
      .limit(1);

    if (!runningServer) {
      throw new Error("Server not found");
    }
    if (runningServer.gameId !== gameId) {
      throw new Error("Server does not belong to this game");
    }
    if (runningServer.gameworldVersionNum !== latestGameworld.versionNum) {
      throw new Error("Server is running an old game version");
    }

    await deleteExpiredReservationsOfRunningServers({
      tx,
      runningServerFilter: sql`request_id = ${requestId}`,
    });

    const [existingRunningPlayerSession] = await tx
      .select({
        requestId: runningPlayerSessions.requestId,
      })
      .from(runningPlayerSessions)
      .where(eq(runningPlayerSessions.profileId, user.id))
      .limit(1);

    const [playerCountResult] = await tx
      .select({ count: count() })
      .from(runningPlayerSessions)
      .where(eq(runningPlayerSessions.requestId, requestId));

    // Check capacity before we drop the player's old seat.
    // Example: if they are moving from server A to full server B, we should keep A.
    const didPlayerAlreadyReserveTargetRunningServer =
      existingRunningPlayerSession?.requestId === requestId;
    if (
      !isSpecificRunningServerAvailableToPlayer({
        playerCount: playerCountResult?.count ?? 0,
        maxPlayers: runningServer.maxPlayers,
        didPlayerAlreadyReserveTargetRunningServer,
      })
    ) {
      throw new Error("Server is full");
    }

    await tx
      .delete(startingPlayerSessions)
      .where(eq(startingPlayerSessions.profileId, user.id));
    await tx
      .delete(runningPlayerSessions)
      .where(eq(runningPlayerSessions.profileId, user.id));

    await tx
      .insert(runningPlayerSessions)
      .values({
        profileId: user.id,
        requestId,
      });

    return savePlayAuth(requestId);
  });
  return {
    requestId: reservedRequestId,
    playAuth,
    thumbnailUuid: await getThumbnailUuid(gameId),
  };
};
