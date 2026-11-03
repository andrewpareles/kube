import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { getBlogPosts } from "../packages/website/lib/blog.ts";
import { readMarkdownPage } from "../packages/website/lib/markdown.ts";
import { repoPath } from "./testPaths.ts";

test("blog posts are markdown files beside the dynamic blog page", () => {
  const blogPostDirPath = repoPath("packages/website/app/blog/[slug]");
  const blogPostFileNames = fs.readdirSync(blogPostDirPath).filter((fileName) => fileName.endsWith(".md"));

  assert.ok(blogPostFileNames.length > 0, "there are no blog posts on disk");
});

test("every blog post carries what its page needs", async () => {
  const blogPosts = await getBlogPosts();
  assert.ok(blogPosts.length > 0);

  for (const blogPost of blogPosts) {
    assert.notEqual(blogPost.title, "");
    assert.notEqual(blogPost.description, "");
    assert.notEqual(blogPost.markdown, "");
    assert.notEqual(blogPost.thumbnailImageUrl, "");
  }
});

test("posts link to the docs page and its anchors, not to old docs routes", () => {
  const blogPostDirPath = repoPath("packages/website/app/blog/[slug]");
  for (const blogPostFileName of fs.readdirSync(blogPostDirPath).filter((fileName) => fileName.endsWith(".md"))) {
    const markdown = fs.readFileSync(`${blogPostDirPath}/${blogPostFileName}`, "utf8");
    assert.equal(markdown.includes("(/docs/"), false, `${blogPostFileName} still links into /docs`);
    assert.equal(markdown.includes("(/api/scripting"), false, `${blogPostFileName} still links to /api/scripting`);
  }
});

test("nothing is left of the old docs section's library", () => {
  assert.equal(fs.existsSync(repoPath("packages/website/lib/docs.ts")), false);
});

test("the privacy policy and the terms are markdown pages that carry what their page needs", async () => {
  for (const pageName of ["privacy", "terms"]) {
    const markdownPage = await readMarkdownPage(repoPath(`packages/website/app/${pageName}/${pageName}.md`));
    assert.notEqual(markdownPage.title, "");
    assert.notEqual(markdownPage.description, "");
    assert.match(markdownPage.markdown, /Last updated/);
  }
});
