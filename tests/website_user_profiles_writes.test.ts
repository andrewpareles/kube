import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const migrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0045_k_coins.sql"), "utf8");
const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const databaseTypesSource = fs.readFileSync(repoPath("packages/website/supabase/database.types.ts"), "utf8");

test("nobody but the server can write to user_profiles", () => {
  assert.match(migrationSource, /revoke update on public\.user_profiles from anon, authenticated;/);
  assert.match(migrationSource, /drop policy "Users can update own profile" on public\.user_profiles;/);
  // The policy has to be gone from the schema too, or the next drizzle push puts it back.
  assert.equal(schemaSource.includes("Users can update own profile"), false);
  assert.match(schemaSource, /pgPolicy\("Users can read own profile"/);
});

test("the unused credits column and the username fallback trigger are gone", () => {
  assert.match(migrationSource, /alter table public\.user_profiles drop column editor_ai_credits;/);
  assert.match(migrationSource, /drop trigger set_default_profile_username on public\.user_profiles;/);
  assert.match(migrationSource, /drop function public\.set_default_profile_username\(\);/);
  assert.equal(schemaSource.includes("editorAiCredits"), false);
  assert.equal(databaseTypesSource.includes("editor_ai_credits"), false);
});
