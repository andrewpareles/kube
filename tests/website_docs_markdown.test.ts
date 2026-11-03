import assert from "node:assert/strict";
import test from "node:test";
import { getDocsMarkdownWithTypeLinks } from "../packages/website/lib/docsMarkdown.ts";

test("docs markdown keeps generated TypeScript angle brackets out of HTML", () => {
  assert.equal(getDocsMarkdownWithTypeLinks("Promise<never>", {}), "Promise\\<never>");
  assert.equal(getDocsMarkdownWithTypeLinks("Promise<ObjectInfo>", { ObjectInfo: "object-info" }), "Promise\\<[ObjectInfo](#object-info)>");
  assert.equal(getDocsMarkdownWithTypeLinks("> Quoted docs", {}), "> Quoted docs");
});

test("docs markdown leaves code spans untouched", () => {
  assert.equal(getDocsMarkdownWithTypeLinks("Use `Promise<never>`.", { Promise: "promise" }), "Use `Promise<never>`.");
});

