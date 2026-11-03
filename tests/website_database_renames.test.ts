import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const renameMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0017_rename_database_objects.sql"), "utf8");
const playerSessionCleanupMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0020_fix_running_player_session_cleanup_after_renames.sql"), "utf8");
const migrationJournalSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/meta/_journal.json"), "utf8");
const databaseTypesSource = fs.readFileSync(repoPath("packages/website/supabase/database.types.ts"), "utf8");
const edgegapActionsSource = fs.readFileSync(repoPath("packages/website/app/server-actions/edgegap-actions.ts"), "utf8");
const serverTrackingSource = fs.readFileSync(repoPath("packages/website/lib/serverTracking.ts"), "utf8");
const serverDatabaseSource = fs.readFileSync(repoPath("crates/server/src/database.rs"), "utf8");
const serverNetworkingSource = fs.readFileSync(repoPath("crates/server/src/server_networking.rs"), "utf8");

test("database rename migration moves current tables and game metadata world version", () => {
  assert.match(renameMigrationSource, /alter table public\.game_likes rename to user_likes/);
  assert.match(renameMigrationSource, /alter table public\.profiles rename to user_profiles/);
  assert.match(renameMigrationSource, /alter table public\."uploads-metadata" rename to data_metadata/);
  assert.match(renameMigrationSource, /alter table public\.gameworlds rename to game_worlds/);
  assert.match(renameMigrationSource, /alter table public\."games-metadata" rename to game_metadata/);
  assert.match(renameMigrationSource, /rename column gameworld_version_num to world_version/);
  assert.match(renameMigrationSource, /insert into public\.user_profiles \(id, email\)/);
  assert.match(renameMigrationSource, /from public\.data_metadata data_row/);
  assert.match(renameMigrationSource, /from public\.game_worlds game_world_row/);
});

test("drizzle schema and generated database types use renamed database objects", () => {
  assert.match(schemaSource, /export const userProfiles = pgTable\("user_profiles"/);
  assert.match(schemaSource, /export const dataMetadata = pgTable\("data_metadata"/);
  assert.match(schemaSource, /export const gameWorlds = pgTable\("game_worlds"/);
  assert.match(schemaSource, /export const games = pgTable\("game_metadata"/);
  assert.equal(schemaSource.includes('worldVersion: bigint("world_version"'), false);
  assert.match(schemaSource, /export const userLikes = pgTable\("user_likes"/);
  assert.equal(schemaSource.includes('pgTable("profiles"'), false);
  assert.equal(schemaSource.includes('pgTable("game_likes"'), false);
  assert.equal(schemaSource.includes('pgTable("gameworlds"'), false);
  assert.equal(schemaSource.includes('pgTable("games-metadata"'), false);
  assert.equal(schemaSource.includes('pgTable("uploads-metadata"'), false);
  assert.match(databaseTypesSource, /game_metadata: \{/);
  assert.equal(databaseTypesSource.includes("world_version: number"), false);
  assert.match(databaseTypesSource, /user_likes: \{/);
  assert.match(databaseTypesSource, /game_worlds: \{/);
  assert.match(databaseTypesSource, /user_profiles: \{/);
  assert.match(databaseTypesSource, /data_metadata: \{/);
});

test("website and Rust database calls use renamed objects", () => {
  assert.match(edgegapActionsSource, /select 1 from user_profiles where id =/);
  assert.match(edgegapActionsSource, /select 1 from game_metadata where game_id =/);
  assert.match(serverTrackingSource, /from user_profiles/);
  assert.match(serverDatabaseSource, /profile:user_profiles!inner\(user_id/);
  assert.match(serverDatabaseSource, /postgrest::from\("game_worlds"\)/);
});

test("post-rename player session cleanup uses renamed profile table", () => {
  assert.match(playerSessionCleanupMigrationSource, /create or replace function public\.delete_running_player_session_unless_play_auth_changed/);
  assert.match(playerSessionCleanupMigrationSource, /from public\.user_profiles/);
  assert.equal(playerSessionCleanupMigrationSource.includes("from public.profiles"), false);
  assert.match(migrationJournalSource, /0020_fix_running_player_session_cleanup_after_renames/);
  assert.match(serverDatabaseSource, /"delete_running_player_session_unless_play_auth_changed"/);
});

test("server disconnect cleanup always attempts player session cleanup", () => {
  assert.match(serverNetworkingSource, /failed to remove player object/);
  assert.equal(serverNetworkingSource.includes("player object {:?} not found in world for user"), false);
  assert.match(serverNetworkingSource, /end_player_session\(profile_id\.as_str\(\), play_auth_hash\.as_str\(\)\)\.await/);
});
