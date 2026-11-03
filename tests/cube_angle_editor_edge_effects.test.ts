import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");

test("depth shading darkens opaque faces from back to front and shades within each receding face", () => {
  assert.match(editorSource, /const faces = faceVertexIndices\.map\([^\n]+\.sort\(\(faceA, faceB\) => faceA\.depth - faceB\.depth\);/);
  assert.match(editorSource, /const shadeAmount = shouldUseDepthShading \? depthShadingAmount \* \(1 - depthProgress\) : 0;/);
  assert.match(editorSource, /const gradientShadeRange = faceDepthGradientAmount \* currentFaceDepthRange \/ vertexDepthRange;/);
  assert.match(editorSource, /const farFaceColor = getShadedFaceColor\(Math\.min\(1, shadeAmount \+ gradientShadeRange \/ 2\)\);/);
  assert.match(editorSource, /const nearFaceColor = getShadedFaceColor\(Math\.max\(0, shadeAmount - gradientShadeRange \/ 2\)\);/);
  assert.match(editorSource, /<linearGradient id="\$\{faceGradientId\}" gradientUnits="userSpaceOnUse"[^`]+<stop offset="0" stop-color="\$\{farFaceColor\}" \/><stop offset="1" stop-color="\$\{nearFaceColor\}" \/>/);
  assert.match(editorSource, /fill="\$\{facePaint\}" stroke="\$\{facePaint\}"/);
});

test("depth shading leaves regular visible edges fully opaque", () => {
  const visibleEdgeSource = editorSource.match(/const visibleEdgeSource = [^\n]+/)?.[0];
  const visibleVertexSource = editorSource.match(/const visibleVertexSource = [^\n]+/)?.[0];
  assert.ok(visibleEdgeSource);
  assert.ok(visibleVertexSource);
  assert.match(visibleEdgeSource, /<path d="\$\{visiblePathData\}"[^>]+stroke-linejoin="round" \/><path d="\$\{visibleSurfaceDetailPathData\}"[^>]+opacity="\$\{movingOpacity\}" \/><path d="\$\{visibleFadingSurfaceDetailPathData\}"[^>]+opacity="\$\{stationaryOpacity\}" \/>/);
  assert.equal(visibleVertexSource.includes("opacity"), false);
});

test("neon glow affects only visible edges and remains embedded in SVG output", () => {
  assert.match(editorSource, /<filter id="\$\{glowId\}"[^`]+<feGaussianBlur in="SourceAlpha"[^`]+result="outer-blur"[^`]+<feGaussianBlur in="SourceAlpha"[^`]+result="inner-blur"[^`]+<feFlood[^`]+<feComposite[^`]+<feMerge>/);
  assert.match(editorSource, /const glowInnerRadius = roundCoordinate\(glowRadius \/ 3\);/);
  assert.match(editorSource, /const visibleSource = shouldUseNeonGlow \? `<g filter="url\(#\$\{glowId\}\)">/);
  assert.doesNotMatch(editorSource.match(/const hiddenSource = [^\n]+/)?.[0] ?? "", /filter=/);
  assert.doesNotMatch(editorSource.match(/const hiddenSurfaceDetailSource = [^\n]+/)?.[0] ?? "", /filter=/);
});

test("stroke gradient uses the selected colors and angle across the SVG view box", () => {
  assert.match(editorSource, /<linearGradient id="\$\{gradientId\}" gradientUnits="userSpaceOnUse"/);
  assert.match(editorSource, /<stop offset="0" stop-color="\$\{gradientStartColor\}" \/><stop offset="1" stop-color="\$\{gradientEndColor\}" \/>/);
  assert.match(editorSource, /const gradientAngleRadians = gradientAngle \* Math\.PI \/ 180;/);
  assert.match(editorSource, /const strokePaint = shouldUseStrokeGradient \? `url\(#\$\{gradientId\}\)` : strokeColor;/);
  assert.match(editorSource, /strokeColorInput\.disabled = strokeGradientInput\.checked;/);
});

test("edge effects reach preview, gallery, export, and saved configuration", () => {
  assert.equal((editorSource.match(/getEdgeEffects\("main-preview"\)/g) ?? []).length, 1);
  assert.equal((editorSource.match(/getEdgeEffects\(`gallery-\$\{effectName\}`\)/g) ?? []).length, 1);
  assert.equal((editorSource.match(/getEdgeEffects\("cube-k"\)/g) ?? []).length, 1);
  assert.match(editorSource, /version: 6/);
  for (const configurationKey of ["shouldUseTransparentFaces", "shouldUseDepthShading", "depthShadingAmount", "faceDepthGradientAmount", "shouldUseNeonGlow", "glowRadius", "glowIntensity", "glowColor", "shouldUseStrokeGradient", "gradientStartColor", "gradientEndColor", "gradientAngle"]) {
    assert.match(editorSource, new RegExp(`${configurationKey}:`));
  }
});

test("face depth gradients remain camera-relative and fade with morph shading", () => {
  assert.match(editorSource, /depthGradientX = \(depthB \* screenCY - depthC \* screenBY\) \/ determinant;/);
  assert.match(editorSource, /depthGradientY = \(screenBX \* depthC - screenCX \* depthB\) \/ determinant;/);
  assert.match(editorSource, /edgeEffects\.faceDepthGradientAmount \*= depthShadingScale;/);
  assert.match(editorSource, /faceDepthGradientAmountInput\.disabled = transparentFacesInput\.checked \|\| !depthShadingInput\.checked;/);
});

test("effect gallery uses the selected face color", () => {
  assert.match(editorSource, /const faceColor = faceColorInput\.value;/);
  assert.equal(editorSource.includes('const faceColor = "#ffffff"'), false);
});

test("transparent faces omit face paths without changing edge visibility math", () => {
  assert.match(editorSource, /<input id="transparent-faces" type="checkbox" \/>/);
  assert.match(editorSource, /const faceSource = shouldUseTransparentFaces \? "" : visibleFaceTriangles\.map/);
  assert.match(editorSource, /shouldUseTransparentFaces: transparentFacesInput\.checked/);
  assert.match(editorSource, /\["shouldUseTransparentFaces", transparentFacesInput\]/);
  assert.match(editorSource, /transparentFacesInput\.addEventListener\("change", updateCube\);/);
});
