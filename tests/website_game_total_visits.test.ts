import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const migrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0014_rename_total_visits.sql"), "utf8");
const edgegapActionsSource = fs.readFileSync(repoPath("packages/website/app/server-actions/edgegap-actions.ts"), "utf8");

test("games store total visits", () => {
  assert.match(schemaSource, /totalVisits: bigint\("total_visits", \{ mode: "number" \}\)\.notNull\(\)\.default\(0\),/);
  assert.match(migrationSource, /rename column total_views to total_visits/);
});

test("successful play reservations increment total visits", () => {
  const incrementText = "totalVisits: sql`${games.totalVisits} + 1`,";

  assert.equal(edgegapActionsSource.split(incrementText).length - 1, 2);
  assert.ok(edgegapActionsSource.indexOf(incrementText) > edgegapActionsSource.indexOf("export const getJoinGameInfoAnyGame"));
  assert.ok(edgegapActionsSource.lastIndexOf(incrementText) > edgegapActionsSource.indexOf("export const getJoinGameInfoSpecificGame"));
});
