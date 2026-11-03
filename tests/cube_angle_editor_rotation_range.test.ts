import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");

test("normal horizontal rotation spans zero through one hundred eighty degrees", () => {
  assert.match(editorSource, /id="horizontal-angle" type="range" min="0" max="180" value="45"/);
  assert.match(editorSource, /horizontalAngleInput\.min = isFullRotation \? "-180" : "0";/);
  assert.match(editorSource, /horizontalAngleInput\.max = "180";/);
  assert.match(editorSource, /Math\.min\(180, Math\.max\(0, Number\(horizontalAngleInput\.value\)\)\)/);
  assert.match(editorSource, /\["horizontalAngle", horizontalAngleInput, configuration\.fullRotation \? -180 : 0, 180\]/);
});

test("horizontal rotation labels match the active range", () => {
  assert.match(editorSource, /horizontalAngleNoteStart\.textContent = isFullRotation \? "−180°" : "0°";/);
  assert.match(editorSource, /horizontalAngleNoteMiddle\.textContent = isFullRotation \? "0°" : "90°";/);
  assert.match(editorSource, /horizontalAngleNoteEnd\.textContent = "180°";/);
});
