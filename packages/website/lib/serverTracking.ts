import { eq, sql } from "drizzle-orm";
import { db } from "@/lib/db";
import { runningServers, startingServers } from "@/supabase/migrations/schema";

export const deleteTrackedServer = async ({
  requestId,
}: {
  requestId: string;
}) => {
  await db.transaction(async (tx) => {
    await tx
      .delete(startingServers)
      .where(eq(startingServers.requestId, requestId));
    await tx
      .delete(runningServers)
      .where(eq(runningServers.requestId, requestId));
  });
};

export const promoteStartingServerToRunning = async ({
  requestId,
  publicIp,
  externalPort,
}: {
  requestId: string;
  publicIp: string;
  externalPort: number;
}) => {
  return db.transaction(async (tx) => {
    const [startingServer] = await tx
      .select({
        requestId: startingServers.requestId,
        gameId: startingServers.gameId,
        gameworldVersionNum: startingServers.gameworldVersionNum,
        maxPlayers: startingServers.maxPlayers,
      })
      .from(startingServers)
      .where(eq(startingServers.requestId, requestId))
      .limit(1);

    if (!startingServer) {
      return null;
    }

    // Lock the same player rows that allocation locks before we move seats.
    // Example: if a player is moving from starting server A to starting server B while A becomes ready,
    // we must wait here so we do not leave running_player_sessions(profile, A) and
    // starting_player_sessions(profile, B) at the same time.
    // The same lock also stops promotion from overwriting a newer running_player_sessions(profile, B) row back to A.
    // It comes before the running_servers insert, which locks the game row through its foreign key, because a launch locks
    // its player before the game, and the other order deadlocks with a player pressing Play again.
    await tx.execute(sql`
      select id
      from user_profiles
      where id in (
        select profile_id
        from starting_player_sessions
        where request_id = ${requestId}
      )
      order by id
      for update
    `);

    await tx
      .insert(runningServers)
      .values({
        requestId: startingServer.requestId,
        gameId: startingServer.gameId,
        gameworldVersionNum: startingServer.gameworldVersionNum,
        maxPlayers: startingServer.maxPlayers,
        publicIp,
        externalPort,
      })
      .onConflictDoUpdate({
        target: runningServers.requestId,
        set: {
          publicIp,
          externalPort,
        },
      });

    await tx.execute(sql`
      insert into running_player_sessions (profile_id, request_id)
      select profile_id, request_id
      from starting_player_sessions
      where request_id = ${requestId}
      on conflict (profile_id) do nothing
    `);

    await tx
      .delete(startingServers)
      .where(eq(startingServers.requestId, requestId));

    return startingServer;
  });
};
