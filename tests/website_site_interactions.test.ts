import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const globalsSource = fs.readFileSync(repoPath("packages/website/app/globals.css"), "utf8");
const siteHeaderSource = fs.readFileSync(repoPath("packages/website/components/site/SiteHeader.tsx"), "utf8");
const gameDetailPageSource = fs.readFileSync(repoPath("packages/website/app/games/[game_id]/GameDetailPage.tsx"), "utf8");

test("enabled buttons and links use the pointer cursor across the website", () => {
  assert.match(globalsSource, /a\[href\],\s*button:not\(:disabled\) \{\s*cursor: pointer;/);
  assert.equal(siteHeaderSource.includes("hover:cursor-pointer"), false);
  assert.equal(gameDetailPageSource.includes("hover:cursor-pointer"), false);
});

