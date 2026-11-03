import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const databaseTypesSource = fs.readFileSync(repoPath("packages/website/supabase/database.types.ts"), "utf8");
const siteGamesSource = fs.readFileSync(repoPath("packages/website/lib/siteGames.ts"), "utf8");
const s3StorageFilesSource = fs.readFileSync(repoPath("packages/website/lib/s3StorageFiles.ts"), "utf8");
const dropMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0018_drop_thumbnail_url.sql"), "utf8");
const thumbnailUploadSource = fs.readFileSync(repoPath("crates/editor_server/src/publishing.rs"), "utf8");

test("game thumbnails are S3 files that the game row points at by uuid", () => {
  assert.equal(schemaSource.includes("thumbnailUrl"), false);
  assert.equal(schemaSource.includes("thumbnail_url"), false);
  assert.equal(databaseTypesSource.includes("thumbnail_url"), false);
  assert.match(dropMigrationSource, /alter table public\.game_metadata drop column thumbnail_url/);
  assert.equal(siteGamesSource.includes("games.thumbnailUrl"), false);
  assert.match(schemaSource, /screenshotUuid: uuid\("screenshot_uuid"\)/);
  assert.match(schemaSource, /aiThumbnailUuid: uuid\("ai_thumbnail_uuid"\)/);
  assert.match(siteGamesSource, /thumbnailUuid: gameRow\.thumbnailUuid/);
  assert.match(s3StorageFilesSource, /urlOfGameThumbnail = \(thumbnailUuid: string \| null\) => thumbnailUuid === null \? null : urlOfPublicFile\(\{ kind: 'GameThumbnail', thumbnailUuid \}\)/);
  assert.equal(thumbnailUploadSource.includes("THUMBNAILS_BUCKET"), false);
});
