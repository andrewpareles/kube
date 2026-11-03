import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const createMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0011_game_likes.sql"), "utf8");
const renameMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0017_rename_database_objects.sql"), "utf8");
const databaseTypesSource = fs.readFileSync(repoPath("packages/website/supabase/database.types.ts"), "utf8");
const siteGamesSource = fs.readFileSync(repoPath("packages/website/lib/siteGames.ts"), "utf8");
const gameLikesActionsSource = fs.readFileSync(repoPath("packages/website/app/server-actions/game-likes.ts"), "utf8");

test("game likes are stored as one profile row per game", () => {
  assert.match(schemaSource, /export const userLikes = pgTable\("user_likes"/);
  assert.match(schemaSource, /primaryKey\(\{ columns: \[table\.gameId, table\.profileId\], name: "user_likes_pkey" \}\)/);
  assert.match(schemaSource, /index\("user_likes_profile_id_idx"\)\.on\(table\.profileId\)/);
  assert.match(createMigrationSource, /create table public\.game_likes/);
  assert.match(renameMigrationSource, /alter table public\.game_likes rename to user_likes/);
  assert.match(renameMigrationSource, /rename constraint game_likes_pkey to user_likes_pkey/);
  assert.match(renameMigrationSource, /rename constraint game_likes_game_id_fkey to user_likes_game_id_fkey/);
  assert.match(renameMigrationSource, /rename constraint game_likes_profile_id_fkey to user_likes_profile_id_fkey/);
  assert.match(databaseTypesSource, /user_likes: \{/);
  assert.match(databaseTypesSource, /foreignKeyName: "user_likes_game_id_fkey"/);
  assert.match(databaseTypesSource, /foreignKeyName: "user_likes_profile_id_fkey"/);
});

test("game queries aggregate real like counts and viewer state", () => {
  assert.match(siteGamesSource, /import \{ gameWorlds, games, runningPlayerSessions, runningServers, siteAdmins, userFavorites, userLikes, userProfiles \}/);
  assert.equal(siteGamesSource.includes("gameFavorites"), false);
  assert.match(siteGamesSource, /const getLikesOfGameId = async/);
  assert.match(siteGamesSource, /likes: count\(userLikes\.profileId\)/);
  assert.match(siteGamesSource, /\.groupBy\(userLikes\.gameId\)/);
  assert.match(siteGamesSource, /likes: likesOfGameId\[gameRow\.gameId\] \?\? 0/);
  assert.match(siteGamesSource, /isLikedByViewer = Boolean\(viewerLike\)/);
  assert.equal(siteGamesSource.includes("likes: 0,"), false);
});

test("like actions toggle rows and revalidate affected pages", () => {
  assert.match(gameLikesActionsSource, /"use server";/);
  assert.match(gameLikesActionsSource, /export const likeGame = async \(gameId: number\)/);
  assert.match(gameLikesActionsSource, /export const unlikeGame = async \(gameId: number\)/);
  assert.match(gameLikesActionsSource, /\.insert\(userLikes\)/);
  assert.match(gameLikesActionsSource, /\.onConflictDoNothing\(\)/);
  assert.match(gameLikesActionsSource, /\.delete\(userLikes\)/);
  assert.match(gameLikesActionsSource, /eq\(userLikes\.gameId, gameId\)/);
  assert.match(gameLikesActionsSource, /eq\(userLikes\.profileId, profileId\)/);
  assert.match(gameLikesActionsSource, /if \(!game\.isPublished && game\.creatorId !== profileId\) \{/);
  assert.equal(gameLikesActionsSource.includes("Creators cannot like their own games"), false);
  assert.match(gameLikesActionsSource, /revalidatePath\(`\/games\/\$\{gameId\}`\)/);
  assert.match(gameLikesActionsSource, /revalidatePath\(`\/profile\/\$\{creatorUserId\}`\)/);
  assert.match(gameLikesActionsSource, /revalidatePath\("\/"\)/);
  assert.match(gameLikesActionsSource, /revalidatePath\("\/create"\)/);
  assert.equal(gameLikesActionsSource.includes('revalidatePath("/games")'), false);
});
