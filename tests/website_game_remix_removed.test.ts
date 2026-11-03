import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const databaseTypesSource = fs.readFileSync(repoPath("packages/website/supabase/database.types.ts"), "utf8");
const siteGamesSource = fs.readFileSync(repoPath("packages/website/lib/siteGames.ts"), "utf8");
const gameSettingsLibSource = fs.readFileSync(repoPath("packages/website/lib/gameSettings.ts"), "utf8");
const gameSettingsActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/game-settings.ts"), "utf8");
const createGameRowSource = fs.readFileSync(repoPath("packages/website/app/server-actions/createGameRow.ts"), "utf8");
const editorDatabaseSource = fs.readFileSync(repoPath("crates/editor_server/src/database.rs"), "utf8");
const dropMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0056_drop_game_remixed_from.sql"), "utf8");

test("remixing is removed from live code and schema", () => {
  for (const source of [schemaSource, databaseTypesSource, siteGamesSource, gameSettingsLibSource, gameSettingsActionSource, createGameRowSource, editorDatabaseSource]) {
    assert.equal(/remix/i.test(source), false);
  }
  assert.match(dropMigrationSource, /drop column remixed_from_game_id/);
  assert.match(dropMigrationSource, /drop column remixed_from_version_num/);
});
