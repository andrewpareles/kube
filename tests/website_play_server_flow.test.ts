import assert from "node:assert/strict";
import test from "node:test";
import {
  getFullestServerWithRoom,
  isSpecificRunningServerAvailableToPlayer,
} from "../packages/website/lib/playServerFlow.ts";

test("getFullestServerWithRoom picks the fullest server that still has room", () => {
  const serverWithRoom = getFullestServerWithRoom([
    {
      requestId: "running-c",
      maxPlayers: 6,
      playerCount: 3,
    },
    {
      requestId: "running-a",
      maxPlayers: 6,
      playerCount: 5,
    },
    {
      requestId: "running-b",
      maxPlayers: 6,
      playerCount: 4,
    },
  ]);

  assert.deepEqual(serverWithRoom, {
    requestId: "running-a",
    maxPlayers: 6,
    playerCount: 5,
  });
});

test("getFullestServerWithRoom skips full servers", () => {
  const serverWithRoom = getFullestServerWithRoom([
    {
      requestId: "running-b",
      maxPlayers: 4,
      playerCount: 4,
    },
    {
      requestId: "running-a",
      maxPlayers: 4,
      playerCount: 3,
    },
  ]);

  assert.deepEqual(serverWithRoom, {
    requestId: "running-a",
    maxPlayers: 4,
    playerCount: 3,
  });
});

test("getFullestServerWithRoom breaks ties by request id", () => {
  const serverWithRoom = getFullestServerWithRoom([
    {
      requestId: "starting-b",
      maxPlayers: 8,
      playerCount: 6,
    },
    {
      requestId: "starting-a",
      maxPlayers: 8,
      playerCount: 6,
    },
  ]);

  assert.deepEqual(serverWithRoom, {
    requestId: "starting-a",
    maxPlayers: 8,
    playerCount: 6,
  });
});

test("getFullestServerWithRoom returns undefined when every server is full", () => {
  const serverWithRoom = getFullestServerWithRoom([
    {
      requestId: "running-a",
      maxPlayers: 2,
      playerCount: 2,
    },
    {
      requestId: "running-b",
      maxPlayers: 5,
      playerCount: 5,
    },
  ]);

  assert.equal(serverWithRoom, undefined);
});

test("isSpecificRunningServerAvailableToPlayer keeps a player on the same full server", () => {
  const isAvailable = isSpecificRunningServerAvailableToPlayer({
    playerCount: 100,
    maxPlayers: 100,
    didPlayerAlreadyReserveTargetRunningServer: true,
  });

  assert.equal(isAvailable, true);
});

test("isSpecificRunningServerAvailableToPlayer rejects a different full server", () => {
  const isAvailable = isSpecificRunningServerAvailableToPlayer({
    playerCount: 100,
    maxPlayers: 100,
    didPlayerAlreadyReserveTargetRunningServer: false,
  });

  assert.equal(isAvailable, false);
});
