import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

test("form controls show focus as a border colour, with no ring or raised shadow", () => {
  // These come back every time a shadcn component is re-copied from upstream, and the ring is
  // what made the boxes look heavy.
  const controlSources = [
    "packages/website/shadcn/components/ui/input.tsx",
    "packages/website/shadcn/components/ui/textarea.tsx",
    "packages/website/shadcn/components/ui/select.tsx",
    "packages/website/app/login/SignInForm.tsx",
    "packages/website/app/login/SignUpForm.tsx",
    "packages/website/app/login/ResetPasswordForm.tsx",
    "packages/website/app/auth/new-password/NewPasswordForm.tsx",
  ];

  for (const controlSource of controlSources) {
    const source = fs.readFileSync(repoPath(controlSource), "utf8");
    assert.equal(/focus(-visible)?:ring-/.test(source), false, `${controlSource} still puts a ring on focus`);
    assert.equal(source.includes("shadow-xs"), false, `${controlSource} still has a raised shadow`);
    assert.match(source, /focus(-visible)?:border-/, `${controlSource} should still change its border on focus`);
  }
});
