import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { GAME_ROW_MAX_WIDTH_PX } from "../packages/website/lib/gameRowLayout.ts";
import { repoPath } from "./testPaths.ts";

const gameRowSource = fs.readFileSync(repoPath("packages/website/components/site/GameRow.tsx"), "utf8");

test("game rows cap at the fixed max shelf width", () => {
  assert.equal(GAME_ROW_MAX_WIDTH_PX, 1248);
  assert.match(gameRowSource, /className="mx-auto w-full" style=\{\{ maxWidth: GAME_ROW_MAX_WIDTH_PX \}\}/);
});
