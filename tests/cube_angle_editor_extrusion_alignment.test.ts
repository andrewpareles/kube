import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const scriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(scriptSource);
const getExtrusionOffsetsSource = scriptSource.match(/const getExtrusionOffsets = \(extrusionDepth, extrusionAlignment\) => \{[\s\S]*?\n      \};/)?.[0];
assert.ok(getExtrusionOffsetsSource);
const getExtrusionOffsets = vm.runInNewContext(`${getExtrusionOffsetsSource}\ngetExtrusionOffsets;`);

test("cube angle editor places partial extrusion at the selected depth alignment", () => {
  assert.deepEqual([...getExtrusionOffsets(0.4, "front")], [0, 0.4]);
  assert.deepEqual([...getExtrusionOffsets(0.4, "center")], [0.3, 0.7]);
  assert.deepEqual([...getExtrusionOffsets(0.4, "back")], [0.6, 1]);
  assert.throws(() => getExtrusionOffsets(0.4, "invalid"), /extrusion alignment is invalid/);
});

test("all extrusion alignments converge at full cube depth", () => {
  for (const extrusionAlignment of ["front", "center", "back"]) {
    assert.deepEqual([...getExtrusionOffsets(1, extrusionAlignment)], [0, 1]);
  }
});

test("alignment reaches the preview, gallery, export, and saved configuration", () => {
  assert.equal((editorSource.match(/getKPrismSvgSource\(kPolygonPoints, "left", extrusionDepth, extrusionAlignmentInput\.value/g) ?? []).length, 3);
  assert.match(editorSource, /extrusionAlignment: extrusionAlignmentInput\.value/);
  assert.match(editorSource, /extrusionAlignmentInput\.value = configuration\.extrusionAlignment/);
});
