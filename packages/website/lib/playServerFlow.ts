export type ServerWithPlayerCount = {
  requestId: string;
  maxPlayers: number;
  playerCount: number;
};

export const getFullestServerWithRoom = <TServer extends ServerWithPlayerCount>(
  serverRows: TServer[],
) => {
  let fullestServerWithRoom: TServer | undefined;
  for (const serverRow of serverRows) {
    if (serverRow.playerCount >= serverRow.maxPlayers) {
      continue;
    }
    if (!fullestServerWithRoom) {
      fullestServerWithRoom = serverRow;
      continue;
    }
    if (serverRow.playerCount > fullestServerWithRoom.playerCount) {
      fullestServerWithRoom = serverRow;
      continue;
    }
    if (
      serverRow.playerCount === fullestServerWithRoom.playerCount &&
      serverRow.requestId.localeCompare(fullestServerWithRoom.requestId) < 0
    ) {
      fullestServerWithRoom = serverRow;
    }
  }
  return fullestServerWithRoom;
};

export const isSpecificRunningServerAvailableToPlayer = ({
  playerCount,
  maxPlayers,
  didPlayerAlreadyReserveTargetRunningServer,
}: {
  playerCount: number;
  maxPlayers: number;
  didPlayerAlreadyReserveTargetRunningServer: boolean;
}) => {
  if (didPlayerAlreadyReserveTargetRunningServer) {
    return true;
  }
  return playerCount < maxPlayers;
};
