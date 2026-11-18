import { NextResponse } from "next/server";
import { and, eq } from "drizzle-orm";
import { db } from "@/lib/db";
import { hashDesktopAuthToken } from "@/lib/desktopAuth";
import {
  getEdgegapDeploymentStatus,
  isTerminalEdgegapStatus,
  readEdgegapConnectionInfo,
  readEdgegapWebtransportInfo,
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

type EdgegapWebtransportInfo = ReturnType<typeof readEdgegapWebtransportInfo>;

// A page needs the browser's address, and a site that cannot name one says why instead of answering with a port the
// browser could not verify.
const readWebtransportOfPort = ({
  requestId,
  webtransportPort,
}: {
  requestId: string;
  webtransportPort: number | null;
}): { webtransport: EdgegapWebtransportInfo } | { error: string } => {
  try {
    return {
      webtransport: readEdgegapWebtransportInfo({ requestId, webtransportPort }),
    };
  } catch (error) {
    return {
      error:
        error instanceof Error
          ? error.message
          : "This site cannot name the game's server.",
    };
  }
};

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
      webtransportPort: runningServers.webtransportPort,
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

    const runningWebtransport = readWebtransportOfPort({
      requestId,
      webtransportPort: runningServer.webtransportPort,
    });
    if ("error" in runningWebtransport) {
      return NextResponse.json(
        { error: runningWebtransport.error },
        { status: 500 },
      );
    }

    return NextResponse.json({
      public_ip: runningServer.publicIp,
      port: runningServer.port,
      webtransport: runningWebtransport.webtransport,
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
    webtransportPort: connectionInfo.webtransportPort,
  });

  const startingWebtransport = readWebtransportOfPort({
    requestId,
    webtransportPort: connectionInfo.webtransportPort,
  });
  if ("error" in startingWebtransport) {
    return NextResponse.json(
      { error: startingWebtransport.error },
      { status: 500 },
    );
  }

  return NextResponse.json({
    public_ip: connectionInfo.publicIp,
    port: connectionInfo.port,
    webtransport: startingWebtransport.webtransport,
  });
}
