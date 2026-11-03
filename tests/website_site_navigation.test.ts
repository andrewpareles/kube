import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";
import { BLOG_PATH, ASSETS_PATH, CATALOG_PATH, CREATE_PATH, DOCS_PATH, GAMES_PATH, PROFILE_PATH, RECENT_GAMES_PATH, TOP_GAMES_PATH, siteResourceNavigationItems, siteTopNavigationItems } from "../packages/website/lib/siteNavigation.ts";

const siteNavigationSource = fs.readFileSync(repoPath("packages/website/lib/siteNavigation.ts"), "utf8");
const siteHeaderSource = fs.readFileSync(repoPath("packages/website/components/site/SiteHeader.tsx"), "utf8");
const siteLayoutShellSource = fs.readFileSync(repoPath("packages/website/app/SiteLayoutShell.tsx"), "utf8");

test("site top navigation links games, new game, profile and the catalog, with the name itself leading home", () => {
  assert.deepEqual(siteTopNavigationItems.map((navigationItem) => navigationItem.label), ["Games", "Profile", "Catalog", "Create"]);
  assert.deepEqual(siteTopNavigationItems.map((navigationItem) => navigationItem.href), [GAMES_PATH, PROFILE_PATH, CATALOG_PATH, CREATE_PATH]);
  assert.equal(GAMES_PATH, "/games");
  // The collections still have their own pages; they are reached from the rows rather than the header.
  assert.equal(TOP_GAMES_PATH, "/games/top");
  assert.equal(RECENT_GAMES_PATH, "/games/recent");
});

test("site resource navigation lists Assets, Docs, and Blog", () => {
  assert.equal(PROFILE_PATH, "/profile");
  assert.equal(siteNavigationSource.includes("Friends"), false);
  // Creating has its own top navigation item, so the resource list does not repeat it.
  assert.deepEqual(siteResourceNavigationItems.map((navigationItem) => navigationItem.label), ["Assets", "Docs", "Blog"]);
  assert.deepEqual(siteResourceNavigationItems.map((navigationItem) => navigationItem.href), [ASSETS_PATH, "/docs", BLOG_PATH]);
  assert.equal(CREATE_PATH, "/create");
  assert.equal(BLOG_PATH, "/blog");
  assert.equal(DOCS_PATH, "/docs");
});

test("site resource navigation uses the code icon for Docs and the book for the blog", () => {
  assert.match(siteNavigationSource, /Docs: Code/);
  // The docs moved into the blog, and their book icon came with them.
  assert.match(siteNavigationSource, /Blog: BookOpen/);
  // Nothing is left of the feed icon the blog used to carry.
  assert.equal(siteNavigationSource.includes("Rss"), false);
});

test("the header carries navigation with a shared menu on smaller screens", () => {
  // Hidden below md, and the room set aside for it goes with it. Which order the classes are
  // written in is not the point, so this only asks that both are on the rail.
  assert.match(siteLayoutShellSource, /<SiteSidebar brand=\{<SiteBrandLink renderId="site-sidebar" logoSidePx=\{56\} \/>\} \/>/);
  assert.match(siteLayoutShellSource, /className="site-body"/);
  // The menu takes its place, and only shows where the sidebar does not.
  assert.match(siteHeaderSource, /aria-label="Open menu"/);
  assert.match(siteHeaderSource, /<Menu className="size-6" \/>/);
  assert.match(siteHeaderSource, /className="site-menu-trigger /);
  // First thing in the row, just left of the mark. On a phone the row holds both ends in by the same
  // amount, since the rail that used to hold the left one shrinks to the mark.
  // Every item on the row keeps px-4 of its own on a phone, so each sits 32px from its neighbour
  // and the ends sit 32px from the screen. Desktop keeps the widths each of them had.
  assert.match(siteHeaderSource, /className="site-header-inner">/);
  assert.match(siteHeaderSource, /<SiteBrandLink renderId="site-header" logoSidePx=\{48\} \/>/);
  assert.match(siteHeaderSource, /"site-nav-link"/);
  assert.match(siteHeaderSource, /className="site-login"/);
  assert.match(siteHeaderSource, /aria-label="Open menu"[\s\S]*?className="site-menu-trigger size-10/);
  assert.ok(
    siteHeaderSource.indexOf("<MobileNavigationMenu />") < siteHeaderSource.indexOf('<SiteBrandLink renderId="site-header"'),
    "the menu is no longer just left of the mark",
  );
  // The settings menu hangs off its button's right edge, so it never opens off the side of a phone.
  assert.equal(siteHeaderSource.match(/align="end"/g)?.length, 1);

  // One list drawn two ways, so the rail and the menu cannot end up with different links or icons.
  assert.match(siteNavigationSource, /export const iconOfResourceLabel/);
  assert.match(siteHeaderSource, /<SiteSidebar brand=\{<SiteBrandLink renderId="site-drawer" logoSidePx=\{48\} \/>\} \/>/);
  // Placeholders stay out of the menu: a row that cannot be pressed is not worth a tap.
  assert.equal(siteHeaderSource.includes("siteSidebarPlaceholderLabels"), false);
});
