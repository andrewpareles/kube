import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const createGameRowSource = fs.readFileSync(repoPath("packages/website/app/server-actions/createGameRow.ts"), "utf8");
const gameSettingsActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/game-settings.ts"), "utf8");
const gameSettingsLibSource = fs.readFileSync(repoPath("packages/website/lib/gameSettings.ts"), "utf8");
const profanityFilterSource = fs.readFileSync(repoPath("packages/website/lib/siteProfanityFilter.ts"), "utf8");
const nextConfigSource = fs.readFileSync(repoPath("packages/website/next.config.ts"), "utf8");
const usernameFilterSource = fs.readFileSync(repoPath("packages/website/lib/siteUsernameFilter.ts"), "utf8");

test("creating and opening a game leave its first world version for Publish", () => {
  const websiteSources = fs
    .globSync("packages/website/**/*.{ts,tsx}", { cwd: repoPath("."), exclude: (path) => path.includes("node_modules") })
    .map((path) => ({ path, source: fs.readFileSync(repoPath(path), "utf8") }));
  assert.deepEqual(websiteSources.filter(({ source }) => /\.insert\(\s*games\s*\)/.test(source)).map(({ path }) => path),
    ["packages/website/app/server-actions/createGameRow.ts"]);
  assert.deepEqual(websiteSources.filter(({ source }) => /\.insert\(\s*gameWorlds\s*\)/.test(source)).map(({ path }) => path), []);
  assert.match(createGameRowSource, /db\.transaction\(async \(tx\) => \{/);
  assert.equal(createGameRowSource.includes("gameWorlds"), false);
});

test("one person cannot make an unbounded number of games", () => {
  // The cap sits in the one place every game is made, so neither the homepage creation path nor
  // the new game dialog can go around it.
  assert.match(createGameRowSource, /const MAX_GAMES_PER_CREATOR = 5000/);
  assert.match(createGameRowSource, /existingGames\.games >= MAX_GAMES_PER_CREATOR/);

  // Counting alone would let two requests at once both read the same number and both through, so
  // the creator's row is taken first, and all of it happens inside the transaction that inserts.
  const lockIndex = createGameRowSource.indexOf(".for('update')");
  const countIndex = createGameRowSource.indexOf("count(games.gameId)");
  const insertIndex = createGameRowSource.indexOf(".insert(games)");
  const transactionIndex = createGameRowSource.indexOf("db.transaction(async (tx) => {");
  assert.ok(lockIndex !== -1);
  assert.ok(transactionIndex < lockIndex);
  assert.ok(lockIndex < countIndex);
  assert.ok(countIndex < insertIndex);
});

test("creating a game takes the creator from the session, never from the caller", () => {
  // createGameRow is a plain server module rather than an action, so the browser cannot reach it
  // and hand it somebody else's creatorId.
  assert.match(createGameRowSource, /^import 'server-only'/m);
  assert.equal(createGameRowSource.includes("'use server'"), false);
  assert.match(gameSettingsActionSource, /export const createGame = async \(\s*newGameSettings: NewGameSettings,\s*\): Promise<\{ gameId: number \}> => \{\s*const creatorId = await getCurrentCreatorId\(\);/);
  assert.equal(/NewGameSettings = \{[^}]*creatorId/s.test(gameSettingsLibSource), false);
});

test("the words on a game are filtered the same way a username is", () => {
  // One matcher for the whole site, so a word blocked in a username cannot be got in as a game
  // title instead.
  // The rules are the `profanity` Rust crate built to WebAssembly, the same one the game server
  // censors chat with, so the site and a running game agree on what counts as a bad word.
  assert.match(profanityFilterSource, /import \{ hasProfanity, initSync \} from '\.\/gen\/profanity\/profanity\.js'/);
  // Read from the working directory, because the copy Next puts beside the bundled code is the one
  // it serves to browsers. next.config.ts is what ships the file itself.
  assert.match(profanityFilterSource, /path\.join\(process\.cwd\(\), 'lib\/gen\/profanity\/profanity_bg\.wasm'\)/);
  assert.match(nextConfigSource, /'\/\*\*': \['\.\/lib\/gen\/profanity\/profanity_bg\.wasm'\]/);
  assert.match(profanityFilterSource, /export \{ hasProfanity \}/);
  assert.equal(profanityFilterSource.includes("obscenity"), false);
  // Relative and with the extension, because the username test imports this file under plain
  // Node, which has neither the path aliases nor extensionless resolution that Next gives it.
  assert.match(usernameFilterSource, /import \{ hasProfanity \} from '\.\/siteProfanityFilter\.ts'/);
  assert.equal(usernameFilterSource.includes("new RegExpMatcher"), false);

  // Both the title and the description are checked, on the way in and on every later edit.
  assert.match(gameSettingsLibSource, /if \(hasProfanity\(name\)\) return "That game title is not allowed\.";/);
  assert.match(gameSettingsLibSource, /if \(description !== null && hasProfanity\(description\)\) return "That description is not allowed\.";/);
  assert.equal(gameSettingsLibSource.split("checkForProfanity({").length - 1, 2);

  // Checked before anything is written, in both paths.
  const createIndex = gameSettingsLibSource.indexOf("export const createGameForCreator");
  const updateIndex = gameSettingsLibSource.indexOf("export const updateGameSettingsForCreator");
  assert.ok(gameSettingsLibSource.indexOf("checkForProfanity({", createIndex) < gameSettingsLibSource.indexOf("await createGameRow({", createIndex));
  assert.ok(gameSettingsLibSource.indexOf("checkForProfanity({", updateIndex) < gameSettingsLibSource.indexOf(".update(games)", updateIndex));

  // The word list stays on the server, so it is never shipped to the page.
  assert.equal(profanityFilterSource.includes("'use client'"), false);
});
