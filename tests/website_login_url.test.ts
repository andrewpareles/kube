import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const deeplinkFlowConstsSource = fs.readFileSync(repoPath("packages/website/app/deeplink/deeplinkFlowConsts.ts"), "utf8");
const useGamePlayLaunchSource = fs.readFileSync(repoPath("packages/website/components/site/useGamePlayLaunch.ts"), "utf8");
const browserEditorRouteSource = fs.readFileSync(repoPath("packages/website/app/editor/[game_id]/page.tsx"), "utf8");
const createPageSource = fs.readFileSync(repoPath("packages/website/app/create/CreatePage.tsx"), "utf8");
const loginModalSource = fs.readFileSync(repoPath("packages/website/app/login/LoginModal.tsx"), "utf8");
const googleSignInButtonSource = fs.readFileSync(repoPath("packages/website/app/login/GoogleSignInButton.tsx"), "utf8");
const oauthCallbackSource = fs.readFileSync(repoPath("packages/website/app/auth/oauth-callback/route.ts"), "utf8");
const googleOneTapSource = fs.readFileSync(repoPath("packages/website/app/login/GoogleOneTap.tsx"), "utf8");
const siteLayoutShellSource = fs.readFileSync(repoPath("packages/website/app/SiteLayoutShell.tsx"), "utf8");

test("one helper builds every login url that carries a launch", () => {
  assert.match(deeplinkFlowConstsSource, /export const getLoginUrl = \(\{ next \}: \{ next: string \}\)/);
  assert.match(deeplinkFlowConstsSource, /\?\$\{LOGIN_SEARCH_PARAM\}=\$\{encodeURIComponent\(next\)\}/);
  // Where they were headed is the whole of it. Nothing tags the url with what they had pressed:
  // the destination already says everything the login modal needs to know.
  assert.equal(deeplinkFlowConstsSource.includes("LoginFrom"), false);
  assert.equal(deeplinkFlowConstsSource.includes("&from="), false);
});

test("every launch that stops for an account opens the login modal over the page, or the homepage's from the server", () => {
  assert.match(useGamePlayLaunchSource, /openLoginModal\(\{ next: loginNextUrl \}\)/);
  assert.match(createPageSource, /openLoginModal\(\{ next: CREATE_PATH \}\)/);
  assert.match(browserEditorRouteSource, /getLoginUrl\(\{ next: EDITOR_PATH\(gameId\) \}\)/);
  // None of them builds a login url by hand.
  assert.equal(useGamePlayLaunchSource.includes("LOGIN_SEARCH_PARAM"), false);
  assert.equal(browserEditorRouteSource.includes("LOGIN_SEARCH_PARAM"), false);
});

test("logging in is only ever the modal, never a page of its own", () => {
  assert.equal(fs.existsSync(repoPath("packages/website/app/login/page.tsx")), false);
  assert.match(loginModalSource, /useSearchParams\(\)\.get\(LOGIN_SEARCH_PARAM\)/);
});

test("the Google popup opens on the click itself, before anything is awaited", () => {
  // Browsers block a popup opened after an await, which would send every sign in down the full page redirect.
  const popupOpenIndex = googleSignInButtonSource.indexOf("window.open(");
  assert.ok(popupOpenIndex !== -1);
  assert.ok(popupOpenIndex < googleSignInButtonSource.indexOf("await "));
  assert.match(googleSignInButtonSource, /skipBrowserRedirect: popup !== null/);
});

test("the popup and the page that opened it agree on the channel, and the page only goes to a path on this site", () => {
  assert.match(googleSignInButtonSource, /new BroadcastChannel\(LOGIN_POPUP_NAME\)/);
  assert.match(googleSignInButtonSource, /\$\{LOGIN_POPUP_SEARCH_PARAM\}=1/);
  assert.match(oauthCallbackSource, /new BroadcastChannel\('\$\{LOGIN_POPUP_NAME\}'\)/);
  assert.match(oauthCallbackSource, /searchParams\.has\(LOGIN_POPUP_SEARCH_PARAM\)/);
  // next comes straight from the url, so anyone could put another site there.
  assert.match(googleSignInButtonSource, /window\.location\.assign\(getRedirectToUrl\(next\)\)/);
});

test("Google One Tap shows only while signed out and off the editor, and Google gets the nonce's hash while Supabase gets the nonce", () => {
  assert.match(siteLayoutShellSource, /\{user === null && <GoogleOneTap \/>\}/);
  // The editor returns before the rest of the site is drawn, One Tap included.
  assert.ok(siteLayoutShellSource.indexOf("return children") < siteLayoutShellSource.indexOf("<GoogleOneTap />"));
  // Static pages are drawn for nobody, so the browser's session is checked before prompting.
  assert.ok(googleOneTapSource.indexOf("if (session !== null) return") < googleOneTapSource.indexOf("google.accounts.id.prompt()"));
  // Swapping these makes every One Tap sign in fail Supabase's nonce check.
  assert.match(googleOneTapSource, /nonce: hashedNonce,/);
  assert.match(googleOneTapSource, /signInWithIdToken\(\{ provider: 'google', token: credential, nonce \}\)/);
  // Google's default drops the request on any click on the page, which hid the picker.
  assert.match(googleOneTapSource, /cancel_on_tap_outside: false,/);
});
