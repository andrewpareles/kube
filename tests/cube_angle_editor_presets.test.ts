import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const colorConfigurationSource = editorSource.match(/const colorConfigurationOfPresetName = (\{[\s\S]*?\n      \});/)?.[1];
const kShapeConfigurationSource = editorSource.match(/const kShapeConfigurationOfPresetName = (\{[\s\S]*?\n      \});/)?.[1];
assert.ok(colorConfigurationSource);
assert.ok(kShapeConfigurationSource);
const colorConfigurationOfPresetName = structuredClone(vm.runInNewContext(`(${colorConfigurationSource})`));
const kShapeConfigurationOfPresetName = structuredClone(vm.runInNewContext(`(${kShapeConfigurationSource})`));

test("the default top endpoint comes from the deep-top wide-stem K", () => {
  assert.match(editorSource, /<input id="parallel-top" type="checkbox" checked \/>/);
  assert.match(editorSource, /<output id="a1-output" for="a1">0\.60<\/output>/);
  assert.match(editorSource, /<input id="a1" type="range" min="0" max="1" value="0\.60"/);
});

test("stroke width starts at two, accepts zero, and has the requested presets", () => {
  assert.match(editorSource, /<output id="stroke-width-output" for="stroke-width">2\.0<\/output>/);
  assert.match(editorSource, /<input id="stroke-width" type="range" min="0" max="24" value="2" step="0\.5" \/>/);
  assert.match(editorSource, /\["strokeWidth", strokeWidthInput, 0, 24\]/);
  for (const strokeWidth of ["0", "0.5", "1", "1.5", "2", "2.5", "3", "4", "5", "6", "7", "8", "10", "12", "14", "16", "18", "20"]) {
    assert.match(editorSource, new RegExp(`data-stroke-width="${strokeWidth}"`));
  }
  assert.match(editorSource, /strokeWidthInput\.value = strokeWidthPresetButton\.dataset\.strokeWidth;\s+updateCube\(\);/);
  assert.ok(editorSource.indexOf('id="k-shape-presets-title"') < editorSource.indexOf('id="stroke-width-presets-title"'));
});

test("color presets contain appearance only and preserve K geometry", () => {
  const appearanceKeys = ["depthShadingAmount", "faceColor", "faceDepthGradientAmount", "glowColor", "glowIntensity", "glowRadius", "gradientAngle", "gradientEndColor", "gradientStartColor", "shouldUseDepthShading", "shouldUseNeonGlow", "shouldUseStrokeGradient", "shouldUseTransparentFaces", "showHiddenEdges", "strokeColor", "strokeWidth"];
  for (const colorConfiguration of Object.values(colorConfigurationOfPresetName)) {
    assert.deepEqual(Object.keys(colorConfiguration).sort(), appearanceKeys);
  }

  const colorHandlerSource = editorSource.match(/for \(const colorPresetButton[\s\S]*?(?=      for \(const kShapePresetButton)/)?.[0];
  assert.ok(colorHandlerSource);
  assert.match(colorHandlerSource, /colorConfigurationOfPresetName\[colorPresetButton\.dataset\.colorPreset\]/);
  assert.match(colorHandlerSource, /Object\.entries\(inputOfColorConfigurationKey\)/);
  assert.doesNotMatch(colorHandlerSource, /parallelTop|parallelBottom|alignInletStarts|inputOfParameter|applyConfigurationText/);
});

test("the supplied electric violet appearance is exact", () => {
  assert.deepEqual(colorConfigurationOfPresetName["electric-violet"], {
    strokeWidth: 6, strokeColor: "#4c00ff", faceColor: "#ffffff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: true, glowRadius: 6, glowIntensity: 0.25, glowColor: "#2f6bff", shouldUseStrokeGradient: true, gradientStartColor: "#4c00ff", gradientEndColor: "#31c7ff", gradientAngle: 42, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender"], {
    strokeWidth: 2, strokeColor: "#4c00ff", faceColor: "#f4f0ff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: true, glowRadius: 6, glowIntensity: 0.25, glowColor: "#2f6bff", shouldUseStrokeGradient: true, gradientStartColor: "#4c00ff", gradientEndColor: "#31c7ff", gradientAngle: 42, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold"], {
    strokeWidth: 8, strokeColor: "#4c00ff", faceColor: "#f4f0ff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: true, glowRadius: 6, glowIntensity: 0.25, glowColor: "#2f6bff", shouldUseStrokeGradient: true, gradientStartColor: "#4c00ff", gradientEndColor: "#31c7ff", gradientAngle: 42, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold-tight-glow"], { ...colorConfigurationOfPresetName["electric-lavender-bold"], glowRadius: 1.5 });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold-tight-glow-hidden"], { ...colorConfigurationOfPresetName["electric-lavender-bold-hidden"], glowRadius: 1.5 });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-crisp"], {
    strokeWidth: 8, strokeColor: "#4c00ff", faceColor: "#f4f0ff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: true, glowRadius: 1.5, glowIntensity: 0.25, glowColor: "#336dff", shouldUseStrokeGradient: true, gradientStartColor: "#4c00ff", gradientEndColor: "#33a7ff", gradientAngle: 42, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold-hidden"], {
    strokeWidth: 8, strokeColor: "#4c00ff", faceColor: "#f4f0ff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: true, glowRadius: 6, glowIntensity: 0.25, glowColor: "#2f6bff", shouldUseStrokeGradient: true, gradientStartColor: "#4c00ff", gradientEndColor: "#31c7ff", gradientAngle: 42, showHiddenEdges: true,
  });
});

test("color presets apply their own stroke width", () => {
  assert.equal(colorConfigurationOfPresetName["electric-violet"].strokeWidth, 6);
  assert.equal(colorConfigurationOfPresetName["electric-violet-hidden"].strokeWidth, 6);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold"].strokeWidth, 8);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold-tight-glow"].strokeWidth, 8);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold-tight-glow-hidden"].strokeWidth, 8);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-crisp"].strokeWidth, 8);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold-hidden"].strokeWidth, 8);
  for (const presetName of ["starter-blue", "electric-lavender", "soft-violet", "soft-violet-hidden", "mono-outline", "golden-depth", "blue-depth", "soft-gray-depth"]) {
    assert.equal(colorConfigurationOfPresetName[presetName].strokeWidth, 2);
  }
  assert.match(editorSource, /strokeWidth: strokeWidthInput,/);
});

test("neon glow starts enabled at twenty percent intensity", () => {
  assert.match(editorSource, /<input id="neon-glow" type="checkbox" checked \/>/);
  assert.match(editorSource, /<output id="glow-intensity-output" for="glow-intensity">0\.20<\/output>/);
  assert.match(editorSource, /<input id="glow-intensity" type="range" min="0" max="1" value="0\.2"/);
  assert.equal(colorConfigurationOfPresetName["starter-blue"].shouldUseNeonGlow, true);
  assert.equal(colorConfigurationOfPresetName["starter-blue"].glowIntensity, 0.2);
});

test("the supplied mono outline and golden depth appearances are exact", () => {
  assert.deepEqual(colorConfigurationOfPresetName["mono-outline"], {
    strokeWidth: 2, strokeColor: "#000000", faceColor: "#ffffff", shouldUseTransparentFaces: false, shouldUseDepthShading: false, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: false, glowRadius: 6, glowIntensity: 0.65, glowColor: "#2f6bff", shouldUseStrokeGradient: false, gradientStartColor: "#002c94", gradientEndColor: "#31c7ff", gradientAngle: 45, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["golden-depth"], {
    strokeWidth: 2, strokeColor: "#000000", faceColor: "#ffc800", shouldUseTransparentFaces: false, shouldUseDepthShading: true, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: false, glowRadius: 6, glowIntensity: 0.1, glowColor: "#000000", shouldUseStrokeGradient: false, gradientStartColor: "#002c94", gradientEndColor: "#31c7ff", gradientAngle: 45, showHiddenEdges: false,
  });
  assert.deepEqual(colorConfigurationOfPresetName["mono-outline-thin"], { ...colorConfigurationOfPresetName["mono-outline"], strokeWidth: 1 });
  assert.deepEqual(colorConfigurationOfPresetName["mono-outline-thick"], { ...colorConfigurationOfPresetName["mono-outline"], strokeWidth: 6 });
});

test("base color presets hide rear edges while the violet copies show them", () => {
  for (const presetName of ["starter-blue", "electric-violet", "electric-lavender", "electric-lavender-bold", "electric-lavender-bold-tight-glow", "electric-lavender-crisp", "soft-violet", "mono-outline", "mono-outline-thin", "mono-outline-thick", "golden-depth", "blue-depth", "soft-gray-depth"]) {
    assert.equal(colorConfigurationOfPresetName[presetName].showHiddenEdges, false);
    assert.equal(colorConfigurationOfPresetName[presetName].shouldUseTransparentFaces, false);
  }
  assert.deepEqual(colorConfigurationOfPresetName["electric-violet-hidden"], { ...colorConfigurationOfPresetName["electric-violet"], shouldUseTransparentFaces: true, showHiddenEdges: true });
  assert.deepEqual(colorConfigurationOfPresetName["soft-violet-hidden"], { ...colorConfigurationOfPresetName["soft-violet"], shouldUseTransparentFaces: true, showHiddenEdges: true });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold-hidden"], { ...colorConfigurationOfPresetName["electric-lavender-bold"], showHiddenEdges: true });
  assert.deepEqual(colorConfigurationOfPresetName["electric-lavender-bold-tight-glow-hidden"], { ...colorConfigurationOfPresetName["electric-lavender-bold-tight-glow"], showHiddenEdges: true });
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold-hidden"].showHiddenEdges, true);
  assert.equal(colorConfigurationOfPresetName["electric-lavender-bold-hidden"].shouldUseTransparentFaces, false);
  assert.equal(colorConfigurationOfPresetName["golden-depth-hidden"], undefined);
});

test("soft violet tones down the electric violet gradient and glow", () => {
  assert.deepEqual(colorConfigurationOfPresetName["soft-violet"], {
    ...colorConfigurationOfPresetName["electric-violet"], strokeWidth: 2, strokeColor: "#40249e", glowRadius: 4, glowIntensity: 0.12, glowColor: "#6680e3", gradientStartColor: "#40249e", gradientEndColor: "#367e96",
  });
  assert.ok(colorConfigurationOfPresetName["soft-violet"].glowIntensity < colorConfigurationOfPresetName["electric-violet"].glowIntensity);
});

test("the supplied blue and soft gray depth appearances are exact", () => {
  const sharedDepthAppearance = { strokeWidth: 2, strokeColor: "#000000", shouldUseTransparentFaces: false, shouldUseDepthShading: true, depthShadingAmount: 0.35, faceDepthGradientAmount: 0.15, shouldUseNeonGlow: false, glowRadius: 6, glowIntensity: 0.65, glowColor: "#2f6bff", shouldUseStrokeGradient: false, gradientStartColor: "#002c94", gradientEndColor: "#31c7ff", gradientAngle: 45, showHiddenEdges: false };
  assert.deepEqual(colorConfigurationOfPresetName["blue-depth"], { ...sharedDepthAppearance, faceColor: "#61adff" });
  assert.deepEqual(colorConfigurationOfPresetName["soft-gray-depth"], { ...sharedDepthAppearance, faceColor: "#f2f2f2" });
});

test("K shape presets contain the current and supplied geometry exactly", () => {
  assert.deepEqual(kShapeConfigurationOfPresetName["current-k"], {
    parallelTop: false, parallelBottom: true, alignInletStarts: false, parameters: { a0: 0.26, a1: 0.54, b0: 0.44, c0: 0.29, c1: 0.56, d0: 0.29, e0: 0.46, e1: 0.5 },
  });
  assert.deepEqual(kShapeConfigurationOfPresetName["aligned-k"], {
    parallelTop: true, parallelBottom: true, alignInletStarts: true, parameters: { a0: 0.26, a1: 0.65, b0: 0.35, c0: 0.26, c1: 0.61, d0: 0.26, e0: 0.55, e1: 0.5 },
  });
  assert.deepEqual(kShapeConfigurationOfPresetName["compact-aligned-k"], {
    parallelTop: true, parallelBottom: true, alignInletStarts: true, parameters: { a0: 0.26, a1: 0.59, b0: 0.3, c0: 0.26, c1: 0.55, d0: 0.26, e0: 0.55, e1: 0.5 },
  });
  assert.deepEqual(kShapeConfigurationOfPresetName["wide-stem-k"], {
    parallelTop: true, parallelBottom: true, alignInletStarts: true, parameters: { a0: 0.33, a1: 0.56, b0: 0.29, c0: 0.33, c1: 0.48, d0: 0.19, e0: 0.39, e1: 0.5 },
  });
  assert.deepEqual(kShapeConfigurationOfPresetName["wide-stem-k-deep-top"], {
    parallelTop: true, parallelBottom: true, alignInletStarts: true, parameters: { a0: 0.33, a1: 0.6, b0: 0.34, c0: 0.33, c1: 0.5, d0: 0.22, e0: 0.39, e1: 0.5 },
  });

  const kShapeHandlerSource = editorSource.match(/for \(const kShapePresetButton[\s\S]*?(?=      for \(const effectCard)/)?.[0];
  assert.ok(kShapeHandlerSource);
  assert.match(kShapeHandlerSource, /kShapeConfigurationOfPresetName\[kShapePresetButton\.dataset\.kShapePreset\]/);
  assert.match(kShapeHandlerSource, /inputOfParameter\[parameterName\]\.value/);
  assert.doesNotMatch(kShapeHandlerSource, /strokeColorInput|faceColorInput|inputOfColorConfigurationKey|applyConfigurationText/);
});

test("the higher-bottom wide stem K raises only the bottom inlet", () => {
  assert.deepEqual(kShapeConfigurationOfPresetName["wide-stem-k-higher-bottom"], {
    parallelTop: true, parallelBottom: true, alignInletStarts: true, parameters: { a0: 0.33, a1: 0.56, b0: 0.29, c0: 0.33, c1: 0.5, d0: 0.22, e0: 0.39, e1: 0.5 },
  });

  const baseParameters = kShapeConfigurationOfPresetName["wide-stem-k"].parameters;
  const higherBottomParameters = kShapeConfigurationOfPresetName["wide-stem-k-higher-bottom"].parameters;
  assert.ok(higherBottomParameters.d0 > baseParameters.d0);
  for (const parameterName of ["a0", "a1", "b0", "c0", "e0", "e1"]) {
    assert.equal(higherBottomParameters[parameterName], baseParameters[parameterName]);
  }
});

test("color, K shape, and stroke presets live in a separate box below the editor", () => {
  assert.ok(editorSource.indexOf("</main>") < editorSource.indexOf('class="preset-gallery"'));
  assert.ok(editorSource.indexOf('id="color-presets-title"') < editorSource.indexOf('id="k-shape-presets-title"'));
  assert.ok(editorSource.indexOf('id="k-shape-presets-title"') < editorSource.indexOf('id="stroke-width-presets-title"'));
  assert.ok(editorSource.indexOf('id="stroke-width-presets-title"') < editorSource.indexOf('class="effect-gallery"'));
  assert.match(editorSource, /\.preset-gallery,\s+\.effect-gallery \{\s+width: min\(1360px, 100%\);/);
  assert.match(editorSource, /\.preset-grid \{\s+display: grid;\s+grid-template-columns: repeat\(3, minmax\(0, 1fr\)\);/);
  for (const presetName of ["starter-blue", "electric-violet", "electric-lavender", "electric-lavender-bold", "electric-lavender-bold-tight-glow", "electric-lavender-bold-tight-glow-hidden", "electric-lavender-crisp", "electric-lavender-bold-hidden", "soft-violet", "mono-outline", "mono-outline-thin", "mono-outline-thick", "golden-depth", "blue-depth", "soft-gray-depth"]) {
    assert.match(editorSource, new RegExp(`\\[data-color-preset=\\"${presetName}\\"\\]`));
  }
});
