import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const hiddenSource = editorSource.match(/const hiddenSource = `[^`]+`;/)?.[0];
const hiddenSurfaceDetailSource = editorSource.match(/const hiddenSurfaceDetailSource = `[^`]+`;/)?.[0];
const hiddenFadingSurfaceDetailSource = editorSource.match(/const hiddenFadingSurfaceDetailSource = `[^`]+`;/)?.[0];
assert.ok(hiddenSource);
assert.ok(hiddenSurfaceDetailSource);
assert.ok(hiddenFadingSurfaceDetailSource);

test("hidden K edges are solid and lighter than visible edges", () => {
  for (const source of [hiddenSource, hiddenSurfaceDetailSource]) {
    assert.equal(source.includes("stroke-dasharray"), false);
  }
  assert.match(editorSource, /const HIDDEN_EDGE_OPACITY = 0\.18;/);
  assert.match(editorSource, /const hiddenGroupSource = shouldShowHiddenEdges \? `<g opacity="\$\{hiddenEdgeOpacity\}">/);
});

test("hidden K edges seal mesh vertices with circles at the matching stroke width", () => {
  assert.match(editorSource, /const hiddenVertexIndices = new Set\(\);/);
  assert.match(editorSource, /const hiddenSurfaceDetailVertexIndices = new Set\(\);/);
  assert.match(editorSource, /const hiddenVertexSource = \[\.\.\.hiddenVertexIndices\]\.map/);
  assert.match(editorSource, /r="\$\{hiddenStrokeWidth \/ 2\}"/);
  assert.match(editorSource, /const hiddenSurfaceDetailVertexSource = \[\.\.\.hiddenSurfaceDetailVertexIndices\]\.filter/);
  assert.match(editorSource, /r="\$\{hiddenMovingSurfaceDetailStrokeWidth \/ 2\}"/);
});

test("the effect gallery follows the hidden-edge checkbox", () => {
  assert.match(editorSource, /const shouldShowHiddenEdges = showHiddenEdgesInput\.checked \|\| hiddenEdgeGrowReverseCubeMorphEffectNames\.has\(effectName\);/);
  assert.match(editorSource, /getKPrismSvgSource\(kPolygonPoints, "left", extrusionDepth, extrusionAlignmentInput\.value, cubeGeometry, shouldShowHiddenEdges, HIDDEN_EDGE_OPACITY, hiddenEdgeStrokeScale, creaseTransition/);
});

test("hidden-edge stroke scale changes hidden paths and their sealing vertices together", () => {
  assert.match(editorSource, /const hiddenStrokeWidth = roundCoordinate\(strokeWidth \* hiddenEdgeStrokeScale\);/);
  assert.match(editorSource, /const hiddenMovingSurfaceDetailStrokeWidth = roundCoordinate\(movingSurfaceDetailStrokeWidth \* hiddenEdgeStrokeScale\);/);
  assert.match(hiddenSource, /stroke-width="\$\{hiddenStrokeWidth\}"/);
  assert.match(hiddenSurfaceDetailSource, /stroke-width="\$\{hiddenMovingSurfaceDetailStrokeWidth\}"/);
});

test("hidden moving creases grow while stationary creases fade", () => {
  assert.equal(hiddenSource.includes("opacity"), false);
  assert.match(hiddenSurfaceDetailSource, /opacity="\$\{movingOpacity\}"/);
  assert.match(hiddenFadingSurfaceDetailSource, /stroke-width="\$\{hiddenStationarySurfaceDetailStrokeWidth\}"/);
  assert.match(hiddenFadingSurfaceDetailSource, /opacity="\$\{stationaryOpacity\}"/);
  assert.match(editorSource, /r="\$\{hiddenStrokeWidth \/ 2\}" fill="\$\{strokePaint\}" \/>/);
  assert.match(editorSource, /r="\$\{hiddenMovingSurfaceDetailStrokeWidth \/ 2\}" fill="\$\{strokePaint\}" opacity="\$\{movingOpacity\}" \/>/);
});
