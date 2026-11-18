import { NextResponse } from "next/server";
import { deleteTrackedServer, promoteStartingServerToRunning } from "@/lib/serverTracking";
import { findEdgegapWebhookRequestId } from "@/lib/edgegapWebhookPayload";

export const EDGEGAP_GAME_PORT_NAME = "gameport";
export const EDGEGAP_WEBTRANSPORT_PORT_NAME = "webtransport";
export const EDGEGAP_GAME_ID_ENV_KEY = "GAME_ID";
export const EDGEGAP_GAMEWORLD_VERSION_NUM_ENV_KEY = "GAMEWORLD_VERSION_NUM";
export const EDGEGAP_GAME_ID_TAG_PREFIX = "g=";
export const EDGEGAP_GAMEWORLD_VERSION_NUM_TAG_PREFIX = "gwv=";
export const EDGEGAP_MAX_PLAYERS_TAG_PREFIX = "mp=";
export const EDGEGAP_DEPLOY_VERSION_NAME = "prod";
export type EdgegapWebhookLifecycleEvent = "ready" | "error" | "terminated";

type EdgegapPortInfo = {
  external: number | null;
  internal: number | null;
  protocol: string;
  name: string | null;
  tls_upgrade: boolean | null;
  link: string | null;
  proxy: number | null;
};

type EdgegapPortsByName = {
  [portName: string]: EdgegapPortInfo;
};

export type EdgegapDeploymentCreateResponse = {
  request_id: string;
};

export type EdgegapDeploymentStatus = {
  request_id: string;
  fqdn: string | null;
  current_status: string;
  running: boolean;
  error: boolean;
  error_detail: string | null;
  public_ip: string | null;
  ports: EdgegapPortsByName | null;
};

export const getRequiredEnvVar = (envKey: string) => {
  const envValue = process.env[envKey];
  if (!envValue) {
    throw new Error(`Missing required env var ${envKey}`);
  }
  return envValue;
};

// The secrets the website passes to each server it starts. Each one has the same name in the website and the server.
const secretEnvKeysOfServer = {
  gameServer: [
    "SUPABASE_SECRET_KEY",
    "S3_ACCESS_KEY_ID",
    "S3_SECRET_ACCESS_KEY",
    "QUIC_SERVER_KEY_DER_BASE64",
    "WEBTRANSPORT_CERT_CHAIN_PEM_BASE64",
    "WEBTRANSPORT_PRIVATE_KEY_PEM_BASE64",
  ],
  editorServer: [
    "EDITOR_SERVER_SECRET",
    "SUPABASE_SECRET_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "RESEND_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "S3_ACCESS_KEY_ID",
    "S3_SECRET_ACCESS_KEY",
    "QUIC_SERVER_KEY_DER_BASE64",
  ],
};

export const getSecretEnvVars = (server: keyof typeof secretEnvKeysOfServer) =>
  secretEnvKeysOfServer[server].map((key) => ({
    key,
    value: getRequiredEnvVar(key),
    is_hidden: true,
  }));

const parseIntEnvVar = (envKey: string) => {
  const envValue = getRequiredEnvVar(envKey);
  const parsedValue = Number.parseInt(envValue, 10);
  if (Number.isNaN(parsedValue)) {
    throw new Error(`${envKey} must be an integer, got ${envValue}`);
  }
  return parsedValue;
};

export const getEdgegapInternalPort = () =>
  parseIntEnvVar("EDGEGAP_INTERNAL_PORT");

export const getEdgegapWebtransportInternalPort = () =>
  parseIntEnvVar("WEBTRANSPORT_INTERNAL_PORT");

// A page dials the browser endpoint by name, so each deployment needs a name of its own under the suffix its
// certificate covers.
export const getWebtransportHostname = (requestId: string) =>
  `${requestId}.${getRequiredEnvVar("WEBTRANSPORT_HOST_SUFFIX")}`;

export const getEdgegapAuthorizationHeader = () => {
  return `token ${getRequiredEnvVar("EDGEGAP_API_TOKEN")}`;
};

export const getEdgegapWebhookUrl = ({
  lifecycleEvent,
}: {
  lifecycleEvent: EdgegapWebhookLifecycleEvent;
}) => {
  const edgegapWebhookBaseUrl = getRequiredEnvVar("EDGEGAP_WEBHOOK_BASE_URL")
    .trim()
    .replace(/\/+$/, "");
  const edgegapWebhookSecret = encodeURIComponent(
    getRequiredEnvVar("EDGEGAP_WEBHOOK_SECRET"),
  );
  return `${edgegapWebhookBaseUrl}/api/edgegap/webhooks/${lifecycleEvent}?secret=${edgegapWebhookSecret}`;
};

export const getEdgegapDeploymentTags = ({
  gameId,
  gameworldVersionNum,
  maxPlayers,
}: {
  gameId: number;
  gameworldVersionNum: number;
  maxPlayers: number;
}) => {
  return [
    `${EDGEGAP_GAME_ID_TAG_PREFIX}${gameId}`,
    `${EDGEGAP_GAMEWORLD_VERSION_NUM_TAG_PREFIX}${gameworldVersionNum}`,
    `${EDGEGAP_MAX_PLAYERS_TAG_PREFIX}${maxPlayers}`,
  ];
};

const parseEdgegapUdpPort = ({
  ports,
  portName,
  edgegapInternalPort,
}: {
  ports: EdgegapPortsByName | null;
  portName: string;
  edgegapInternalPort: number;
}) => {
  const namedPort = ports?.[portName];
  if (namedPort?.external && namedPort.protocol.toUpperCase().includes("UDP")) {
    return namedPort.external;
  }

  for (const portInfo of Object.values(ports ?? {})) {
    if (!portInfo.external) {
      continue;
    }
    if (!portInfo.protocol.toUpperCase().includes("UDP")) {
      continue;
    }
    if (portInfo.internal !== edgegapInternalPort) {
      continue;
    }
    return portInfo.external;
  }

  return null;
};

export const hasEdgegapConnectionInfo = ({
  publicIp,
  externalPort,
}: {
  publicIp: string | null;
  externalPort: number | null;
}) => {
  return Boolean(publicIp?.trim() && externalPort);
};

// Read the current live deployment status from Edgegap when we need fresh connection info.
export const getEdgegapDeploymentStatus = async ({
  requestId,
}: {
  requestId: string;
}) => {
  const response = await fetch(`https://api.edgegap.com/v1/status/${requestId}`, {
    method: "GET",
    headers: {
      Authorization: getEdgegapAuthorizationHeader(),
      "Content-Type": "application/json",
    },
    cache: "no-store",
  });

  if (response.status === 404) {
    return null;
  }

  if (!response.ok) {
    const responseBody = await response.text();
    throw new Error(
      `Edgegap request failed (${response.status} ${response.statusText}): ${responseBody}`,
    );
  }

  return (await response.json()) as EdgegapDeploymentStatus;
};

// Pull the address data we need from the current Edgegap status payload: the desktop client's QUIC port, and the
// browser's HTTP/3 port, which is published separately because one UDP port cannot serve both.
export const readEdgegapConnectionInfo = (
  deploymentStatus: EdgegapDeploymentStatus,
) => {
  const publicIp = deploymentStatus.public_ip?.trim() ?? null;
  const port = parseEdgegapUdpPort({
    ports: deploymentStatus.ports,
    portName: EDGEGAP_GAME_PORT_NAME,
    edgegapInternalPort: getEdgegapInternalPort(),
  });
  if (!deploymentStatus.running || !publicIp || port === null) {
    return null;
  }

  return {
    requestId: deploymentStatus.request_id,
    publicIp,
    port,
    webtransportPort: parseEdgegapUdpPort({
      ports: deploymentStatus.ports,
      portName: EDGEGAP_WEBTRANSPORT_PORT_NAME,
      edgegapInternalPort: getEdgegapWebtransportInternalPort(),
    }),
  };
};

// What a browser needs to join: the name its certificate covers, on the HTTP/3 port Edgegap published for it, and the
// certificate's hash when no authority signed it.
export const readEdgegapWebtransportInfo = ({
  requestId,
  webtransportPort,
}: {
  requestId: string;
  webtransportPort: number | null;
}) => {
  if (webtransportPort === null) {
    return null;
  }
  return {
    host: getWebtransportHostname(requestId),
    port: webtransportPort,
    certificate_sha256_base64:
      process.env.WEBTRANSPORT_CERTIFICATE_SHA256_BASE64 || null,
  };
};

export const isTerminalEdgegapStatus = (currentStatus: string) => {
  const currentStatusLower = currentStatus.toLowerCase();
  return (
    currentStatusLower.includes("error") ||
    currentStatusLower.includes("failed") ||
    currentStatusLower.includes("terminated") ||
    currentStatusLower.includes("stopped") ||
    currentStatusLower.includes("deleted") ||
    currentStatusLower.includes("removed")
  );
};

export const handleEdgegapWebhookPost = async ({
  request,
  lifecycleEvent,
}: {
  request: Request;
  lifecycleEvent: EdgegapWebhookLifecycleEvent;
}) => {
  const requestUrl = new URL(request.url);
  const requestSecret = requestUrl.searchParams.get("secret");
  if (requestSecret !== getRequiredEnvVar("EDGEGAP_WEBHOOK_SECRET")) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const requestBody = await request.text();
  let requestJson: unknown;
  try {
    requestJson = JSON.parse(requestBody);
  } catch {
    return NextResponse.json(
      { error: "Invalid JSON payload" },
      { status: 400 },
    );
  }

  // make sure we have requestId
  const requestId = findEdgegapWebhookRequestId(requestJson);
  if (!requestId) {
    console.warn("Edgegap webhook payload missing request_id", { lifecycleEvent, requestBody, });
    return NextResponse.json({ error: "Missing request_id" }, { status: 400 },);
  }
  console.log(`Edgegap game server webhook ${lifecycleEvent} for ${requestId}`);


  // handle the lifecycle event
  if (lifecycleEvent === "ready") {
    const deploymentStatus = await getEdgegapDeploymentStatus({ requestId });
    if (!deploymentStatus) {
      return NextResponse.json({ success: true });
    }
    const connectionInfo = readEdgegapConnectionInfo(deploymentStatus);
    if (connectionInfo) {
      await promoteStartingServerToRunning({
        requestId,
        publicIp: connectionInfo.publicIp,
        externalPort: connectionInfo.port,
      });
    }
    if (!deploymentStatus.error && !isTerminalEdgegapStatus(deploymentStatus.current_status)) {
      return NextResponse.json({ success: true });
    }
  }

  if (lifecycleEvent === "error") {
    await deleteTrackedServer({
      requestId,
    });
  }

  if (lifecycleEvent === "terminated") {
    await deleteTrackedServer({
      requestId,
    });
  }

  return NextResponse.json({ success: true });
};
