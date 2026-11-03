import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const { passwordMinLength } = await import("../packages/website/lib/sitePasswords.ts");

const signUpFormSource = fs.readFileSync(repoPath("packages/website/app/login/SignUpForm.tsx"), "utf8");
const signInFormSource = fs.readFileSync(repoPath("packages/website/app/login/SignInForm.tsx"), "utf8");
const signUpActionSource = fs.readFileSync(repoPath("packages/website/app/server-actions/sign-up.ts"), "utf8");

test("one number decides how long a password has to be, and both the form and the action read it", () => {
  assert.equal(passwordMinLength, 8);
  assert.match(signUpFormSource, /minLength=\{passwordMinLength\}/);
  // However it is worded, the number in it is read rather than typed out, so it cannot go stale.
  assert.match(signUpFormSource, /placeholder=\{`[^`]*\$\{passwordMinLength\}[^`]*`\}/);
  assert.match(signUpActionSource, /if \(password\.length < passwordMinLength\)/);
  // The form's check is gone by the time the button is pressed, so the action has to be the one
  // that decides. It runs before anything is asked of Supabase.
  const passwordCheckIndex = signUpActionSource.indexOf("password.length < passwordMinLength");
  assert.ok(passwordCheckIndex !== -1);
  assert.ok(passwordCheckIndex < signUpActionSource.indexOf("await supabase.auth.signUp"));
});

test("signing in is not held to the new minimum, so older passwords still get in", () => {
  assert.equal(signInFormSource.includes("passwordMinLength"), false);
  assert.match(signInFormSource, /minLength=\{6\}/);
});
