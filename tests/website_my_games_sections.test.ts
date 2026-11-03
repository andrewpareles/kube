import assert from "node:assert/strict";
import test from "node:test";
import { getMyGamesSplitByPublishState } from "../packages/website/lib/myGamesSections.ts";

test("getMyGamesSplitByPublishState separates published and private games in order", () => {
  const games = [
    { gameId: 1, isPublished: true },
    { gameId: 2, isPublished: false },
    { gameId: 3, isPublished: true },
    { gameId: 4, isPublished: false },
  ];

  const { publishedGames, privateGames } = getMyGamesSplitByPublishState(games);

  assert.deepEqual(publishedGames.map((game) => game.gameId), [1, 3]);
  assert.deepEqual(privateGames.map((game) => game.gameId), [2, 4]);
});
