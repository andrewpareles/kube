import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const migrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0044_game_chats.sql"), "utf8");
const migrationJournalSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/meta/_journal.json"), "utf8");
const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const serverDatabaseSource = fs.readFileSync(repoPath("crates/server/src/database.rs"), "utf8");

// The server writes these rows straight to PostgREST, so a column the table does not have is a
// request that fails in production and nowhere else. These three have to say the same thing.
test("the columns the server writes chat to are the columns the table has", () => {
  const columnNames = ["serverid", "time_live_server", "userid", "message"];
  const writerStruct = serverDatabaseSource.slice(
    serverDatabaseSource.indexOf("struct GameChatRow"),
    serverDatabaseSource.indexOf("let serverid = ARBITRIUM_REQUEST_ID"),
  );
  assert.notEqual(writerStruct, "");
  for (const columnName of columnNames) {
    assert.match(writerStruct, new RegExp(`\\b${columnName}\\b`));
    assert.match(migrationSource, new RegExp(`^\\s*${columnName} `, "m"));
  }
  assert.match(migrationSource, /create table public\.game_chats/);
  assert.match(migrationJournalSource, /0044_game_chats/);
  assert.match(schemaSource, /export const gameChats = pgTable\("game_chats"/);
});

// A log nobody can read is not worth keeping, and the row has to outlive the server that wrote it.
test("chat rows are private, searchable, and survive the server that wrote them", () => {
  assert.match(migrationSource, /alter table public\.game_chats enable row level security/);
  assert.match(migrationSource, /create policy "Service role only" on public\.game_chats/);
  assert.match(migrationSource, /create index game_chats_serverid_time_idx/);
  assert.match(migrationSource, /create index game_chats_userid_time_idx/);
  // running_servers rows are deleted on shutdown, so a reference to one would take the log with it.
  assert.equal(migrationSource.includes("references public.running_servers"), false);
});
