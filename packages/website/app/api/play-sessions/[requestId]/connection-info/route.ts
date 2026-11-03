import { NextResponse } from "next/server";
import { and, eq } from "drizzle-orm";
import { db } from "@/lib/db";
import { hashDesktopAuthToken } from "@/lib/desktopAuth";
import {
  getEdgegapDeploymentStatus,
  isTerminalEdgegapStatus,
  readEdgegapConnectionInfo,
} from "@/lib/edgegapApi";
import {
  deleteTrackedServer,
  promoteStartingServerToRunning,
} from "@/lib/serverTracking";
import {
  userDesktopPlayAuth,
  runningPlayerSessions,
  runningServers,
  startingPlayerSessions,
} from "@/supabase/migrations/schema";

export const runtime = "nodejs";

type RouteContext = {
  params: Promise<{
    requestId: string;
  }>;
};

export async function GET(request: Request, context: RouteContext) {
  const authorizationHeader = request.headers.get("authorization");
  const playAuth = authorizationHeader?.startsWith("Bearer ")
    ? authorizationHeader.slice("Bearer ".length).trim()
    : "";
  if (!playAuth) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const playAuthHash = hashDesktopAuthToken(playAuth);
  const [playerAuthRow] = await db
    .select({
      profileId: userDesktopPlayAuth.profileId,
    })
    .from(userDesktopPlayAuth)
    .where(eq(userDesktopPlayAuth.playAuthHash, playAuthHash))
    .limit(1);
  if (!playerAuthRow) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const { requestId } = await context.params;

  // Promotion swaps the starting seat for the running server in one transaction, so the starting seat must be read first.
  const [startingPlayerSession] = await db
    .select({
      profileId: startingPlayerSessions.profileId,
    })
    .from(startingPlayerSessions)
    .where(
      and(
        eq(startingPlayerSessions.profileId, playerAuthRow.profileId),
        eq(startingPlayerSessions.requestId, requestId),
      ),
    )
    .limit(1);

  // Return immediately if this request_id has already been promoted to a running server.
  const [runningServer] = await db
    .select({
      publicIp: runningServers.publicIp,
      port: runningServers.externalPort,
    })
    .from(runningServers)
    .where(eq(runningServers.requestId, requestId))
    .limit(1);
  if (runningServer) {
    const [runningPlayerSession] = await db
      .select({
        profileId: runningPlayerSessions.profileId,
      })
      .from(runningPlayerSessions)
      .where(
        and(
          eq(runningPlayerSessions.profileId, playerAuthRow.profileId),
          eq(runningPlayerSessions.requestId, requestId),
        ),
      )
      .limit(1);
    if (!runningPlayerSession) {
      return NextResponse.json(
        { error: "Server is no longer available" },
        { status: 410 },
      );
    }

    return NextResponse.json({
      public_ip: runningServer.publicIp,
      port: runningServer.port,
    });
  }

  // Starting rows are the only rows that are still waiting on Edgegap to become connectable.
  if (!startingPlayerSession) {
    return NextResponse.json(
      { error: "Server is no longer available" },
      { status: 410 },
    );
  }

  let deploymentStatus;
  try {
    deploymentStatus = await getEdgegapDeploymentStatus({ requestId });
  } catch (error) {
    const errorMessage =
      error instanceof Error ? error.message : "Failed loading server status";
    return NextResponse.json({ error: errorMessage }, { status: 502 });
  }

  if (!deploymentStatus) {
    await deleteTrackedServer({ requestId });
    return NextResponse.json(
      { error: "Server is no longer available" },
      { status: 410 },
    );
  }
  if (deploymentStatus.error || isTerminalEdgegapStatus(deploymentStatus.current_status)) {
    await deleteTrackedServer({ requestId });
    return NextResponse.json(
      { error: deploymentStatus.error_detail ?? "Server is no longer available" },
      { status: 410 },
    );
  }

  const connectionInfo = readEdgegapConnectionInfo(deploymentStatus);
  if (!connectionInfo) {
    return NextResponse.json({}, { status: 202 });
  }

  await promoteStartingServerToRunning({
    requestId,
    publicIp: connectionInfo.publicIp,
    externalPort: connectionInfo.port,
  });

  return NextResponse.json({
    public_ip: connectionInfo.publicIp,
    port: connectionInfo.port,
  });
}
