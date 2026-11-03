import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const scriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(scriptSource);
const renderingConstantsSource = scriptSource.match(/const VIEW_BOX_SIZE = 256;[\s\S]*?const HIDDEN_EDGE_OPACITY = 0\.18;/)?.[0];
const renderingFunctionsSource = scriptSource.match(/const roundCoordinate = [\s\S]*?(?=\n      \/\/ -------------------------- Hover studies)/)?.[0];
assert.ok(renderingConstantsSource);
assert.ok(renderingFunctionsSource);
const renderingApi = vm.runInNewContext(`${renderingConstantsSource}\n${renderingFunctionsSource}\n({ getCubeGeometry, getKPolygonPoints, getKPrismSvgSource });`);

test("overlapping faces use triangle depth masks for the reported steep centered view", () => {
  const cubeGeometry = renderingApi.getCubeGeometry(68.3, 44.1);
  const kPolygonPoints = renderingApi.getKPolygonPoints({ a0: 0.26, a1: 0.645, b0: 0.35, c0: 0.26, c1: 0.557, d0: 0.27, e0: 0.55, e1: 0.5 }, 0);
  const svgSource = renderingApi.getKPrismSvgSource(kPolygonPoints, "left", 0.59, "center", cubeGeometry, false, 0.3, 1, { movingStrokeScale: 1, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 1 }, 2, "#000000", "#ffc800", {
    renderId: "steep-centered-regression",
    shouldUseDepthShading: true,
    depthShadingAmount: 0.35,
    faceDepthGradientAmount: 0.15,
    shouldUseNeonGlow: false,
    glowRadius: 6,
    glowIntensity: 0.1,
    glowColor: "#000000",
    shouldUseStrokeGradient: false,
    gradientStartColor: "#002c94",
    gradientEndColor: "#31c7ff",
    gradientAngle: 45,
  });

  assert.match(svgSource, /<mask id="steep-centered-regression-face-occlusion-/);
  assert.match(svgSource, /fill="black" stroke="black"/);
  assert.match(svgSource, /mask="url\(#steep-centered-regression-face-occlusion-/);
});

test("face occlusion compares interpolated triangle depths instead of average face depths", () => {
  assert.match(editorSource, /const faceTriangleIndices = \[frontTriangleIndices,/);
  assert.match(editorSource, /getOccludingTriangleDepth\(screenPoint\) - getFaceTriangleDepth\(screenPoint\)/);
  assert.match(editorSource, /const visibleFaceTriangles = \[\];/);
  assert.doesNotMatch(editorSource, /Opaque faces are sorted from back to front/);
});

test("extruded wall triangles face outward before back-face removal", () => {
  assert.match(editorSource, /const sideTriangleIndices = \[\[pointIndex, pointCount \+ pointIndex, pointCount \+ nextPointIndex\], \[pointIndex, pointCount \+ nextPointIndex, nextPointIndex\]\];/);
  assert.match(editorSource, /faceVertexIndices\.push\(\[pointIndex, pointCount \+ pointIndex, pointCount \+ nextPointIndex, nextPointIndex\]\);/);
});
