import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { getBreadcrumbJsonLd, getJsonLdScriptText } from "../packages/website/lib/siteJsonLd.ts";
import { getNoIndexPageMetadata, getSiteArticleMetadata, getSitePageMetadata } from "../packages/website/lib/siteMetadata.ts";
import { repoPath } from "./testPaths.ts";

const readWebsiteSource = (path: string) => fs.readFileSync(repoPath(`packages/website/${path}`), "utf8");
const layoutSource = readWebsiteSource("app/layout.tsx");
const sitemapSource = readWebsiteSource("app/sitemap.ts");
const robotsSource = readWebsiteSource("app/robots.ts");
const gameRouteSource = readWebsiteSource("app/games/[game_id]/page.tsx");
const profileRouteSource = readWebsiteSource("app/profile/[user_id]/[[...section]]/page.tsx");
const blogPostRouteSource = readWebsiteSource("app/blog/[slug]/page.tsx");
const siteGamesSource = readWebsiteSource("lib/siteGames.ts");
const siteMetadataSource = readWebsiteSource("lib/siteMetadata.ts");
const siteProfilesSource = readWebsiteSource("lib/siteProfiles.ts");
const footerSource = readWebsiteSource("app/SiteFooter.tsx");
const proxySource = readWebsiteSource("proxy.ts");

test("public pages share canonical and social metadata rooted at the production origin", () => {
  assert.match(layoutSource, /metadataBase: new URL\(SITE_ORIGIN\)/);
  assert.match(layoutSource, /template: `%s \| \$\{SITE_NAME\}`/);
  assert.match(layoutSource, /"max-image-preview": "large"/);
  assert.equal(layoutSource.includes("localhost"), false);
  assert.equal(fs.existsSync(repoPath("packages/website/public/opengraph-image.jpg")), true);
  assert.equal(fs.existsSync(repoPath("packages/website/app/opengraph-image.png")), false);
  assert.equal(fs.existsSync(repoPath("packages/website/app/opengraph-image.alt.txt")), false);
});

test("a page's canonical link and its social URLs all come from the one path it was given", () => {
  const metadata = getSitePageMetadata({ title: "Top Games", description: "The most upvoted games on kube.new.", path: "/games/top", imageUrl: "/opengraph-image.png", imageAlt: "the card" });
  assert.deepEqual(metadata.alternates, { canonical: "/games/top" });
  assert.equal(metadata.openGraph?.url, "/games/top");
  assert.equal(metadata.openGraph?.title, "Top Games | kube.new");
  assert.equal(metadata.twitter?.card, "summary_large_image");
  assert.deepEqual(metadata.openGraph?.images, [{ url: "/opengraph-image.png", alt: "the card" }]);
});

test("only articles say they are articles, and only they carry a publication date", () => {
  const articleMetadata = getSiteArticleMetadata({ title: "A Post", description: "What it says.", path: "/blog/a-post", imageUrl: "/thumb.png", imageAlt: "the thumbnail", publishedTime: "2026-08-01" });
  assert.equal(articleMetadata.openGraph?.type, "article");
  assert.equal(articleMetadata.openGraph?.publishedTime, "2026-08-01");
  const pageMetadata = getSitePageMetadata({ title: "Blog", description: "kube.new blog posts.", path: "/blog", imageUrl: "/thumb.png", imageAlt: "the thumbnail" });
  assert.equal(pageMetadata.openGraph?.type, "website");
  assert.equal(pageMetadata.openGraph?.publishedTime, undefined);
});

test("workflow pages stay out of search", () => {
  assert.deepEqual(getNoIndexPageMetadata({ title: "Log in", description: null }).robots, { index: false, follow: false });
});

test("robots and sitemap expose public pages without blocking public API documentation", () => {
  assert.match(robotsSource, /sitemap: getSiteUrl\('\/sitemap\.xml'\)/);
  assert.match(robotsSource, /'\/api\/edgegap\/'/);
  assert.match(robotsSource, /'\/api\/get-client-update-info\/'/);
  assert.match(robotsSource, /'\/api\/play-sessions\/'/);
  assert.match(robotsSource, /'\/api\/stripe\/'/);
  assert.match(robotsSource, /'\/plans\/return'/);
  assert.equal(robotsSource.includes("disallow: '/api'"), false);
  assert.match(sitemapSource, /getRecentGames\(\)/);
  assert.match(sitemapSource, /getBlogPosts\(\)/);
  const staticPathsSource = sitemapSource.split("const staticPaths = [")[1].split("]")[0];
  for (const path of ["CREATE_PATH", "GAMES_PATH", "ASSETS_PATH", "TOP_GAMES_PATH", "RECENT_GAMES_PATH", "BLOG_PATH", "DOCS_PATH", "DOWNLOAD_PATH", "PRIVACY_PATH", "TERMS_PATH"]) {
    assert.equal(staticPathsSource.includes(`${path},`), true, `${path} is missing from the sitemap`);
  }
  assert.match(sitemapSource, /`\/games\/\$\{game\.gameId\}`/);
  assert.match(sitemapSource, /`\$\{PROFILE_PATH\}\/\$\{game\.creatorUserId\}`/);
  assert.match(sitemapSource, /`\$\{BLOG_PATH\}\/\$\{blogPost\.slug\}`/);
  assert.match(proxySource, /robots\\\\\.txt\$/);
  assert.match(proxySource, /sitemap\\\\\.xml\$/);
});

test("every dynamic route emits the structured data for the thing it shows", () => {
  assert.match(gameRouteSource, /'@type': 'VideoGame'/);
  assert.match(profileRouteSource, /'@type': 'ProfilePage'/);
  assert.match(profileRouteSource, /robots: \{ index: false, follow: true \}/);
  assert.match(blogPostRouteSource, /'@type': 'BlogPosting'/);
});

test("JSON-LD escapes less-than signs, so a game name cannot end the script element early", () => {
  assert.equal(getJsonLdScriptText({ name: "</script><b>x" }), '{"name":"\\u003c/script>\\u003cb>x"}');
});

test("breadcrumb JSON-LD derives registered ancestors from one page path", () => {
  assert.deepEqual(getBreadcrumbJsonLd({ path: "/blog/networking", pageName: "Networking" }).itemListElement, [
    { "@type": "ListItem", position: 1, name: "Home", item: "https://kube.new/" },
    { "@type": "ListItem", position: 2, name: "Blog", item: "https://kube.new/blog" },
    { "@type": "ListItem", position: 3, name: "Networking", item: "https://kube.new/blog/networking" },
  ]);
});

test("dynamic breadcrumbs use the page's real name and skip unregistered parents", () => {
  assert.deepEqual(getBreadcrumbJsonLd({ path: "/games/42", pageName: "Block Party" }).itemListElement, [
    { "@type": "ListItem", position: 1, name: "Home", item: "https://kube.new/" },
    { "@type": "ListItem", position: 2, name: "Block Party", item: "https://kube.new/games/42" },
  ]);
  assert.deepEqual(getBreadcrumbJsonLd({ path: "/profile/8", pageName: "@builder" }).itemListElement, [
    { "@type": "ListItem", position: 1, name: "Home", item: "https://kube.new/" },
    { "@type": "ListItem", position: 2, name: "@builder", item: "https://kube.new/profile/8" },
  ]);
});

test("structured-data builders are separate from metadata builders", () => {
  assert.equal(siteMetadataSource.includes("breadcrumbNameOfAncestorPath"), false);
  assert.equal(siteMetadataSource.includes("getBreadcrumbJsonLd"), false);
});

test("profile SEO derives its public games from the existing viewer-visible game query", () => {
  assert.equal(siteProfilesSource.includes("getPublicProfile"), false);
  assert.match(siteGamesSource, /games: SiteGameSummary\[\]/);
  assert.match(siteGamesSource, /const allowedGamesWhere = isOwner/);
  assert.equal(profileRouteSource.includes("getPublicProfile"), false);
});

test("workflow pages are noindex and the global 404 is explicitly noindex", () => {
  for (const path of [
    "app/auth/auth-code-error/page.tsx",
    "app/auth/new-password/page.tsx",
    "app/profile/page.tsx",
    "app/not-found.tsx",
  ]) {
    assert.match(readWebsiteSource(path), /getNoIndexPageMetadata/, `${path} is missing noindex metadata`);
  }
});

test("navigation only advertises real canonical routes", () => {
  for (const footerPath of ["/docs", "/blog", "/privacy", "/terms"]) {
    assert.equal(footerSource.includes(`href="${footerPath}"`), true, `the footer no longer links ${footerPath}`);
    assert.equal(fs.existsSync(repoPath(`packages/website/app${footerPath}/page.tsx`)), true, `${footerPath} is in the footer but has no route`);
  }
  assert.equal(fs.existsSync(repoPath("packages/website/app/blogs")), false);
});

test("every public index and detail route emits applicable structured data", () => {
  for (const path of ["app/create/page.tsx", "app/games/page.tsx", "app/blog/page.tsx", "app/docs/page.tsx", "app/privacy/page.tsx", "app/terms/page.tsx"]) {
    assert.match(readWebsiteSource(path), /getBreadcrumbJsonLd/, `${path} is missing breadcrumbs`);
  }
});
