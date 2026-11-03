import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const {
  getUsernameProblem,
  usernameMaxLength,
  usernameMinLength,
} = await import("../packages/website/lib/siteUsernames.ts");
// The word list is read from the working directory, the same as the blog posts are.
const originalCwd = process.cwd();
process.chdir(repoPath("packages/website"));
const { getUsernameFilterProblem } = await import("../packages/website/lib/siteUsernameFilter.ts");
process.chdir(originalCwd);

const uniqueUsernamesMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0026_unique_usernames.sql"), "utf8");
const usernameMinLengthMigrationSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/0027_username_min_length_four.sql"), "utf8");
const migrationJournalSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/meta/_journal.json"), "utf8");
const schemaSource = fs.readFileSync(repoPath("packages/website/supabase/migrations/schema.ts"), "utf8");
const changeUsernameBannerSource = fs.readFileSync(repoPath("packages/website/app/profile/[user_id]/ChangeUsernameBanner.tsx"), "utf8");
const emailFormStateSource = fs.readFileSync(repoPath("packages/website/app/login/emailFormState.ts"), "utf8");
const availabilityStateSource = fs.readFileSync(repoPath("packages/website/app/profile/[user_id]/usernameAvailabilityState.ts"), "utf8");
const availabilityActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/username-available.ts"), "utf8");
const signUpActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/sign-up.ts"), "utf8");
const changeUsernameActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/change-username.ts"), "utf8");

test("a name is refused for being too short, too long, or written with anything but letters and digits", () => {
  assert.equal(getUsernameProblem("bobm"), null);
  assert.equal(getUsernameProblem("Bob99"), null);
  assert.equal(getUsernameProblem("b".repeat(usernameMaxLength)), null);

  assert.notEqual(getUsernameProblem(""), null);
  assert.notEqual(getUsernameProblem("b".repeat(usernameMinLength - 1)), null);
  assert.notEqual(getUsernameProblem("b".repeat(usernameMaxLength + 1)), null);
  // The first two read as the same name once they are on the page, which is the whole reason the
  // characters are limited.
  assert.notEqual(getUsernameProblem("bob "), null);
  assert.notEqual(getUsernameProblem("b o b"), null);
  assert.notEqual(getUsernameProblem("bob!"), null);
  assert.notEqual(getUsernameProblem("bob_99"), null);
  assert.notEqual(getUsernameProblem("bob-99"), null);
});

test("the rules the form checks are the rules the table checks", () => {
  assert.match(schemaSource, /char_length\(\$\{table\.username\}\) between 4 and 16/);
  assert.match(usernameMinLengthMigrationSource, /check \(char_length\(username\) between 4 and 16\)/);
  assert.match(migrationJournalSource, /0027_username_min_length_four/);
  assert.equal(usernameMinLength, 4);
  assert.equal(usernameMaxLength, 16);
  assert.match(schemaSource, /\^\[A-Za-z0-9\]\+\$/);
  assert.match(uniqueUsernamesMigrationSource, /check \(username ~ '\^\[A-Za-z0-9\]\+\$'\)/);
});

test("no two accounts can hold the same name, whatever case it is written in", () => {
  // The index doing this is older than the migration files and was left under the table's old name,
  // so this migration renames it rather than making a second one that does the same job.
  assert.match(uniqueUsernamesMigrationSource, /alter index public\.profiles_username_lower_key\s*\nrename to user_profiles_username_lower_key/);
  assert.equal(uniqueUsernamesMigrationSource.includes("create unique index"), false);
  assert.match(schemaSource, /uniqueIndex\("user_profiles_username_lower_key"\)\.on\(sql`lower\(\$\{table\.username\}\)`\)/);
  // The lookup has to ignore case the same way, or it would call a name free that sign up then
  // cannot take.
  assert.match(availabilityActionSource, /lower\(\$\{userProfiles\.username\}\) = \$\{username\.toLowerCase\(\)\}/);
});

test("the one account that could have been left called Guest is given a name of its own", () => {
  assert.match(uniqueUsernamesMigrationSource, /set username = 'user' \|\| public_id/);
  assert.match(uniqueUsernamesMigrationSource, /where username = 'Guest'/);
  assert.match(migrationJournalSource, /0026_unique_usernames/);
  // The name it hands out has to pass the rules the same migration goes on to impose, or it would
  // refuse its own backfill.
  assert.equal(getUsernameProblem("user1"), null);
  assert.equal(getUsernameProblem(`user${"9".repeat(usernameMaxLength - 4)}`), null);
});

test("sign up sends no name, since every account starts as Guest-<user_id>", () => {
  assert.equal(emailFormStateSource.includes("username"), false);
  assert.equal(signUpActionSource.includes("username"), false);
});

test("making the account and signing in go through the server", () => {
  // The page hands the two fields to the action; it no longer talks to Supabase itself. Signing
  // in goes through the server too, so the page it lands on is drawn for the new account. Resetting a
  // password has nothing to check against a list, so it stays on the page.
  assert.match(emailFormStateSource, /await createAccount\(\{ email, password, emailRedirectTo: emailRedirectToOnSuccess \}\)/);
  assert.equal(emailFormStateSource.includes("supabase.auth.signUp"), false);
  assert.match(emailFormStateSource, /await signIn\(\{ email, password, next \}\)/);
  assert.equal(emailFormStateSource.includes("signInWithPassword"), false);
  assert.match(emailFormStateSource, /supabase\.auth\.resetPasswordForEmail/);
  assert.match(signUpActionSource, /^"use server";/);
  assert.match(signUpActionSource, /await supabase\.auth\.signUp\(\{/);
});

test("the server checks the name again before renaming the account, not just while it is typed", () => {
  assert.match(changeUsernameActionSource, /^"use server";/);
  assert.match(changeUsernameActionSource, /const availability = await getUsernameAvailability\(username\)/);
  const availabilityIndex = changeUsernameActionSource.indexOf("await getUsernameAvailability(username)");
  const renameIndex = changeUsernameActionSource.indexOf(".set({ username })");
  assert.ok(availabilityIndex !== -1);
  assert.ok(availabilityIndex < renameIndex);
  // Both ways of failing come back on the one channel the form already shows in red.
  assert.match(changeUsernameActionSource, /return \{ type: "error", error: `\$\{username\} is taken\.` \}/);
  assert.match(changeUsernameActionSource, /return \{ type: "error", error: availability\.problem \}/);
  // Only the signed-in account's own row is renamed, and only while it still has its Guest name, so a name is picked once.
  assert.match(changeUsernameActionSource, /\.where\(and\(eq\(userProfiles\.id, user\.id\), sql`\$\{userProfiles\.username\} = 'Guest-' \|\| \$\{userProfiles\.userId\}`\)\)/);
  assert.match(changeUsernameActionSource, /if \(renamedProfiles\.length === 0\) \{\n\t\treturn \{ type: "error", error: "You already picked your username\." \};/);
});

test("a name is only looked up once typing pauses, and a late answer about an old name is dropped", () => {
  assert.match(availabilityStateSource, /const availabilityLookupDelayMs = \d+/);
  assert.match(availabilityStateSource, /window\.setTimeout/);
  assert.match(availabilityStateSource, /return \(\) => window\.clearTimeout\(timeoutId\)/);
  // The answer is kept together with the name it was about, so it can only ever be shown beside
  // that name; a lookup that lands after the name changed no longer matches.
  assert.match(availabilityStateSource, /setCheckedAvailability\(\{ username, availability: await getUsernameAvailability\(username\) \}\)/);
  assert.match(availabilityStateSource, /checkedAvailability\.username !== username\) return \{ type: 'looking' \}/);
  // A name the rules already rule out never reaches the server.
  assert.match(availabilityStateSource, /if \(username === '' \|\| getUsernameProblem\(username\) !== null\) return/);
});

test("the availability lookup applies both the writing rules and the word list before looking anybody up", () => {
  assert.match(availabilityActionSource, /const problem = getUsernameProblem\(username\) \?\? getUsernameFilterProblem\(username\)/);
  const problemIndex = availabilityActionSource.indexOf("getUsernameFilterProblem(username)");
  const lookupIndex = availabilityActionSource.indexOf("await db");
  assert.ok(problemIndex < lookupIndex);
  // A name that breaks a rule is answered with the reason, not with the same word used for a name
  // somebody already holds.
  assert.match(availabilityActionSource, /return \{ type: "problem", problem \}/);
});

test("names that read as the site or as staff are refused", () => {
  assert.notEqual(getUsernameFilterProblem("admin"), null);
  assert.notEqual(getUsernameFilterProblem("Admin"), null);
  assert.notEqual(getUsernameFilterProblem("kube"), null);
  assert.notEqual(getUsernameFilterProblem("support"), null);
  assert.notEqual(getUsernameFilterProblem("games"), null);
  // Matched whole, so a real name that happens to start with one is left alone.
  assert.equal(getUsernameFilterProblem("adminah"), null);
  assert.equal(getUsernameFilterProblem("Modesty"), null);
});

test("crude names are refused, including the spellings used to sneak them past a plain list", () => {
  assert.notEqual(getUsernameFilterProblem("shit"), null);
  assert.notEqual(getUsernameFilterProblem("sh1t"), null);
  assert.notEqual(getUsernameFilterProblem("fvck"), null);
  assert.notEqual(getUsernameFilterProblem("FUCK"), null);
});

test("ordinary names are left alone, including the ones a careless word list would catch", () => {
  assert.equal(getUsernameFilterProblem("Andrew"), null);
  assert.equal(getUsernameFilterProblem("bob99"), null);
  // Every one of these contains a rude word as a run of letters. A filter that flagged them would
  // be turning away real people, which is worse than letting a rude name through.
  assert.equal(getUsernameFilterProblem("Scunthorpe"), null);
  assert.equal(getUsernameFilterProblem("Assassin"), null);
  assert.equal(getUsernameFilterProblem("classic"), null);
  assert.equal(getUsernameFilterProblem("Cockburn"), null);
  assert.equal(getUsernameFilterProblem("analyst"), null);
});

test("the note under the field names the name it is talking about", () => {
  // Every line under the field is a full sentence ending in a full stop, including these two.
  assert.match(changeUsernameBannerSource, /\{username\} is available\./);
  assert.match(changeUsernameBannerSource, /\{username\} is taken\./);
});
