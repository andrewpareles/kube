import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");

test("the desktop columns fit before the responsive layout stacks", () => {
  const trackMinimums = editorSource.match(/grid-template-columns: minmax\((\d+)px, 1fr\) minmax\((\d+)px, (\d+)px\);/);
  const stackingBreakpoint = editorSource.match(/@media \(max-width: (\d+)px\) \{\s+main \{/);

  assert.ok(trackMinimums);
  assert.ok(stackingBreakpoint);

  const minimumDesktopWidth = Number(trackMinimums[1]) + Number(trackMinimums[2]) + 24;
  assert.ok(Number(stackingBreakpoint[1]) >= minimumDesktopWidth);
});

test("desktop controls and preview use the expanded workspace", () => {
  assert.match(editorSource, /width: min\(1360px, 100%\);/);
  assert.match(editorSource, /height: calc\(100dvh - 24px\);/);
  assert.match(editorSource, /grid-template-columns: minmax\(420px, 1fr\) minmax\(480px, 520px\);/);
  assert.match(editorSource, /width: min\(100%, 480px\);/);
  assert.match(editorSource, /scrollbar-gutter: stable;/);
});

test("desktop download and configuration tools remain reachable in the preview scroller", () => {
  const previewSource = editorSource.match(/\.preview \{([\s\S]*?)\n      \}/)?.[1];

  assert.ok(previewSource);
  assert.match(previewSource, /grid-template-rows: minmax\(260px, 1fr\) auto;/);
  assert.match(previewSource, /overflow-y: auto;/);
  assert.ok(editorSource.indexOf('class="preview-case"') < editorSource.indexOf('class="preview-tools"'));
  assert.ok(editorSource.indexOf('class="preview-tools"') < editorSource.indexOf('id="download"'));
  assert.ok(editorSource.indexOf('id="download"') < editorSource.indexOf('class="configuration-tools"'));
  assert.ok(editorSource.indexOf('class="configuration-tools"') < editorSource.indexOf('class="controls"'));
});

test("the main preview hides its scaffolding without an overriding caption style", () => {
  assert.match(editorSource, /cubePath\.style\.opacity = hideCubeGuideInput\.checked \? "0" : String\(CUBE_GUIDE_OPACITY\);/);
  assert.doesNotMatch(editorSource, /cubePath\.setAttribute\("opacity"/);
  assert.doesNotMatch(editorSource, /K on left face/);
});

test("stacked layouts return controls to the page scroll", () => {
  const responsiveSource = editorSource.match(/@media \(max-width: 960px\) \{([\s\S]*?)\n      \}/)?.[1];

  assert.ok(responsiveSource);
  assert.match(responsiveSource, /main \{[\s\S]*?height: auto;[\s\S]*?grid-template-columns: 1fr;[\s\S]*?overflow: visible;/);
  assert.match(responsiveSource, /\.controls \{[\s\S]*?overflow: visible;/);
});
