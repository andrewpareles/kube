import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const originalCwd = process.cwd();
process.chdir(repoPath("packages/website"));
const { formatBlogPostDate, getBlogPostBySlug, getBlogPosts } = await import("../packages/website/lib/blog.ts");
process.chdir(originalCwd);
const blogPageSource = fs.readFileSync(repoPath("packages/website/app/blog/page.tsx"), "utf8");
const blogPostPageSource = fs.readFileSync(repoPath("packages/website/app/blog/[slug]/page.tsx"), "utf8");

test("getBlogPostBySlug reads a markdown post with metadata", async () => {
  // Whichever post is first on the index, rather than one named here that can be deleted later.
  const [newestBlogPost] = await getBlogPosts();
  assert.notEqual(newestBlogPost, undefined);

  const blogPost = await getBlogPostBySlug(newestBlogPost.slug);
  assert.notEqual(blogPost, null);
  if (blogPost === null) return;

  assert.equal(blogPost.slug, newestBlogPost.slug);
  assert.notEqual(blogPost.title, "");
  assert.notEqual(blogPost.description, "");
  assert.match(blogPost.date, /^\d{4}-\d{2}-\d{2}$/);
  // Posts name their author by public profile id, so the @username shown comes from that profile.
  assert.equal(Number.isInteger(blogPost.authorUserId), true);
  assert.notEqual(blogPost.authorName, "");
  assert.match(blogPost.thumbnailImageUrl, /^\/blog\//);
  assert.notEqual(blogPost.markdown, "");
});

test("getBlogPostBySlug returns null for a slug with no post", async () => {
  assert.equal(await getBlogPostBySlug("not-a-post"), null);
});

test("getBlogPosts uses a distinct existing thumbnail for every post", async () => {
  const blogPosts = await getBlogPosts();
  const isThumbnailImageUrlUsed: { [thumbnailImageUrl: string]: true } = {};

  for (const blogPost of blogPosts) {
    assert.equal(isThumbnailImageUrlUsed[blogPost.thumbnailImageUrl], undefined);
    isThumbnailImageUrlUsed[blogPost.thumbnailImageUrl] = true;
    assert.equal(blogPost.thumbnailImageUrl.startsWith("/blog/"), true);
    assert.equal(fs.existsSync(repoPath(`packages/website/public${blogPost.thumbnailImageUrl}`)), true);
  }
});

test("formatBlogPostDate writes the date in English", () => {
  assert.equal(formatBlogPostDate("2026-05-07"), "May 7, 2026");
});

test("blog index owns the blog card listing", () => {
  assert.match(blogPageSource, /title: 'Blog'/);
  assert.match(blogPageSource, /path: '\/blog'/);
  assert.match(blogPageSource, /const blogPosts = await getBlogPosts\(\)/);
  assert.match(blogPageSource, /href: `\/blog\/\$\{blogPost\.slug\}`/);
  assert.match(blogPageSource, /description: blogPost\.description/);
  assert.match(blogPostPageSource, /description: blogPost\.description/);
  assert.equal(blogPageSource.includes("permanentRedirect"), false);
  assert.equal(fs.existsSync(repoPath("packages/website/app/blogs/page.tsx")), false);
});
