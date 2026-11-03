import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const websiteDirPath = repoPath("packages/website");

// Reading the user costs a round trip to the Supabase auth server, so everything asks the one
// cached helper instead of asking Supabase for itself. Only that helper and the cookie plumbing
// underneath it talk to Supabase auth directly.
const pathsAllowedToReadTheUserDirectly = [
  "lib/siteUser.ts",
  "supabase",
];

const getSourceFilePaths = (dirPath: string): string[] => {
  const sourceFilePaths: string[] = [];
  for (const dirEntry of fs.readdirSync(dirPath, { withFileTypes: true })) {
    const entryPath = path.join(dirPath, dirEntry.name);
    if (dirEntry.isDirectory()) {
      if (dirEntry.name === "node_modules" || dirEntry.name === ".next") continue;
      sourceFilePaths.push(...getSourceFilePaths(entryPath));
      continue;
    }
    if (dirEntry.name.endsWith(".ts") || dirEntry.name.endsWith(".tsx")) sourceFilePaths.push(entryPath);
  }
  return sourceFilePaths;
};

test("pages read the user through the cached helper instead of revalidating the token themselves", () => {
  const offendingPaths: string[] = [];
  for (const sourceFilePath of getSourceFilePaths(websiteDirPath)) {
    if (!fs.readFileSync(sourceFilePath, "utf8").includes("auth.getUser()")) continue;
    const websiteRelativePath = path.relative(websiteDirPath, sourceFilePath);
    const isAllowed = pathsAllowedToReadTheUserDirectly.some((allowedPath) => websiteRelativePath.startsWith(allowedPath));
    if (!isAllowed) offendingPaths.push(websiteRelativePath);
  }

  assert.deepEqual(offendingPaths, [], `these should call getUser() instead: ${offendingPaths.join(", ")}`);
});

test("the user helper is cached, so one render never revalidates the token twice", () => {
  const siteUserSource = fs.readFileSync(path.join(websiteDirPath, "lib/siteUser.ts"), "utf8");
  assert.match(siteUserSource, /export const getUser = cache\(/);
});

test("nothing is signed out halfway: the callers that cannot carry on ask for the user with requireUser", () => {
  const siteUserSource = fs.readFileSync(path.join(websiteDirPath, "lib/siteUser.ts"), "utf8");
  assert.match(siteUserSource, /export const requireUser = async/);

  // One owner check, so publishing a game and renaming it cannot disagree about whose game it is.
  const ownerCheckPaths = getSourceFilePaths(websiteDirPath)
    .filter((sourceFilePath) => /const getGameIfOwned = async/.test(fs.readFileSync(sourceFilePath, "utf8")))
    .map((sourceFilePath) => path.relative(websiteDirPath, sourceFilePath));
  assert.deepEqual(ownerCheckPaths, ["lib/getGameIfOwned.ts"]);
});
