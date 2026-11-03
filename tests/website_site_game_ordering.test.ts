import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const siteGamesSource = fs.readFileSync(repoPath("packages/website/lib/siteGames.ts"), "utf8");
const gamesPageSource = fs.readFileSync(repoPath("packages/website/app/games/page.tsx"), "utf8");
const topGamesPageSource = fs.readFileSync(repoPath("packages/website/app/games/top/page.tsx"), "utf8");
const recentGamesPageSource = fs.readFileSync(repoPath("packages/website/app/games/recent/page.tsx"), "utf8");

test("top and recent are ordered by the database, not by the pages that show them", () => {
  assert.match(siteGamesSource, /export const getTopGames = async \(\): Promise<SiteGameSummary\[\]>/);
  assert.match(siteGamesSource, /export const getRecentGames = async \(\): Promise<SiteGameSummary\[\]>/);
  assert.match(siteGamesSource, /\.orderBy\(desc\(sql`coalesce\(\$\{playerCountsOfGame\.players\}, 0\)`\), desc\(sql`coalesce\(\$\{likeCountsOfGame\.likes\}, 0\)`\), desc\(games\.createdAt\)\)/);
  // Nothing sorts games in JavaScript any more, and the helper that used to is gone.
  assert.equal(fs.existsSync(repoPath("packages/website/lib/siteGameSorting.ts")), false);
  assert.equal(siteGamesSource.includes("sortGamesByUpvotes"), false);
});

test("an unliked game cannot come top", () => {
  // The like counts are a left join, so a game nobody liked joins to null. Postgres sorts nulls
  // first on a descending order, which would put every unliked game above every liked one.
  const orderByIndex = siteGamesSource.indexOf("coalesce(${likeCountsOfGame.likes}, 0)");
  assert.ok(orderByIndex !== -1);
  assert.match(siteGamesSource, /\.leftJoin\(likeCountsOfGame, eq\(likeCountsOfGame\.gameId, games\.gameId\)\)/);
});

test("every page asks for the ordering it wants by name", () => {
  // Recommended mixes the newest games in with the top ones, under the staff picks, so the page that shows it asks for all three.
  assert.match(gamesPageSource, /await Promise\.all\(\[getStaffPickGames\(\), getTopGames\(\), getRecentGames\(\)\]\)/);
  assert.match(topGamesPageSource, /const topGames = await getTopGames\(\)/);
  assert.match(recentGamesPageSource, /const recentGames = await getRecentGames\(\)/);
  // The old catch-all is gone, so no page can reach for everything and sort it itself.
  assert.equal(siteGamesSource.includes("getPublishedGames"), false);
});
