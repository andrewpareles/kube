import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const scriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(scriptSource);
const morphConstantsSource = scriptSource.match(/const EFFECT_MORPH_GEOMETRY_END = .*?;/)?.[0];
const morphFunctionSource = scriptSource.match(/const getCubeMorphState = \(boxProgress\) => \(\{[\s\S]*?\n      \}\);/)?.[0];
const creaseTransitionFunctionSource = scriptSource.match(/const getCreaseTransition = \(shouldAnimate, transitionMode, cubeMorphState\) => \{[\s\S]*?\n      \};/)?.[0];
const renderingConstantsSource = scriptSource.match(/const VIEW_BOX_SIZE = 256;[\s\S]*?const HIDDEN_EDGE_OPACITY = 0\.18;/)?.[0];
const renderingFunctionsSource = scriptSource.match(/const roundCoordinate = [\s\S]*?(?=\n      \/\/ -------------------------- Hover studies)/)?.[0];
assert.ok(morphConstantsSource);
assert.ok(morphFunctionSource);
assert.ok(creaseTransitionFunctionSource);
assert.ok(renderingConstantsSource);
assert.ok(renderingFunctionsSource);
const getCubeMorphState = vm.runInNewContext(`${morphConstantsSource}\n${morphFunctionSource}\ngetCubeMorphState;`);
const getCreaseTransition = vm.runInNewContext(`${creaseTransitionFunctionSource}\ngetCreaseTransition;`);
const renderingApi = vm.runInNewContext(`${renderingConstantsSource}\n${renderingFunctionsSource}\n({ getCubeGeometry, getKPolygonPoints, getKPrismSvgSource });`);

test("cube morph grows detail edge stroke width at the same time as the K movement", () => {
  for (const boxProgress of [0, 0.35, 0.7, 0.8, 1]) {
    assert.equal(getCubeMorphState(boxProgress).surfaceDetailStrokeScale, getCubeMorphState(boxProgress).kGeometryScale);
  }
  assert.equal(getCubeMorphState(0).surfaceDetailStrokeScale, 1);
  assert.equal(getCubeMorphState(0.35).surfaceDetailStrokeScale, 0.5);
  assert.equal(getCubeMorphState(0.7).surfaceDetailStrokeScale, 0);
  assert.match(editorSource, /stroke-width="\$\{movingSurfaceDetailStrokeWidth\}" stroke-linecap="butt" stroke-linejoin="round" opacity="\$\{movingOpacity\}" \/>/);
});

test("cube morph moves the inlet points diagonally toward their anchor midpoints", () => {
  const parameterValues = { a0: 0.26, a1: 0.66, b0: 0.44, c0: 0.29, c1: 0.53, d0: 0.25, e0: 0.46, e1: 0.5 };
  const kPoints = renderingApi.getKPolygonPoints(parameterValues, 0);
  const halfwayPoints = renderingApi.getKPolygonPoints({ ...parameterValues, b0: parameterValues.b0 / 2, d0: parameterValues.d0 / 2 }, 0.5);
  const cubePoints = renderingApi.getKPolygonPoints({ ...parameterValues, b0: 0, d0: 0 }, 1);

  assert.deepEqual([...kPoints[2]], [parameterValues.a0, parameterValues.b0]);
  assert.deepEqual([...halfwayPoints[2]], [parameterValues.a0 + (parameterValues.a1 - parameterValues.a0) / 4, parameterValues.b0 / 2]);
  assert.deepEqual([...cubePoints[2]], [(parameterValues.a0 + parameterValues.a1) / 2, 0]);
  assert.deepEqual([...kPoints[8]], [parameterValues.c0, 1 - parameterValues.d0]);
  assert.deepEqual([...halfwayPoints[8]], [parameterValues.c0 + (parameterValues.c1 - parameterValues.c0) / 4, 1 - parameterValues.d0 / 2]);
  assert.deepEqual([...cubePoints[8]], [(parameterValues.c0 + parameterValues.c1) / 2, 1]);
});

test("moving K creases shrink while stationary K creases fade", () => {
  const cubeGeometry = renderingApi.getCubeGeometry(22, 45);
  const kPolygonPoints = renderingApi.getKPolygonPoints({ a0: 0.33, a1: 0.6, b0: 0.34, c0: 0.33, c1: 0.5, d0: 0.22, e0: 0.39, e1: 0.5 }, 0);
  const svgSource = renderingApi.getKPrismSvgSource(kPolygonPoints, "left", 1, "front", cubeGeometry, false, 0.18, 1, { movingStrokeScale: 0.4, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 0.4 }, 10, "#000000", "#ffffff", {
    renderId: "moving-k-detail-regression",
    shouldUseTransparentFaces: false,
    shouldUseDepthShading: false,
    depthShadingAmount: 0.35,
    faceDepthGradientAmount: 0.15,
    shouldUseNeonGlow: false,
    glowRadius: 6,
    glowIntensity: 0.2,
    glowColor: "#000000",
    shouldUseStrokeGradient: false,
    gradientStartColor: "#000000",
    gradientEndColor: "#000000",
    gradientAngle: 45,
  });

  assert.match(svgSource, /<path d="M[^"]+" fill="none" stroke="#000000" stroke-width="4" stroke-linecap="butt" stroke-linejoin="round" opacity="1" \/>/);
  assert.match(svgSource, /<path d="M[^"]+" fill="none" stroke="#000000" stroke-width="10" stroke-linecap="butt" stroke-linejoin="round" opacity="0\.4" \/>/);
  assert.match(editorSource, /const movingKPolygonPointIndices = new Set\(\[2, 5, 8\]\);/);
  assert.doesNotMatch(editorSource, /isSurfaceDetail: !simplifiedKPolygonPointIndexSet\.has/);
});

test("crease transition controls offer every requested animation treatment", () => {
  const halfwayMorphState = getCubeMorphState(0.35);
  assert.deepEqual({ ...getCreaseTransition(true, "fade", halfwayMorphState) }, { movingStrokeScale: 1, movingOpacity: 0.5, stationaryStrokeScale: 1, stationaryOpacity: 0.5 });
  assert.deepEqual({ ...getCreaseTransition(true, "shrink", halfwayMorphState) }, { movingStrokeScale: 0.5, movingOpacity: 1, stationaryStrokeScale: 0.5, stationaryOpacity: 1 });
  assert.deepEqual({ ...getCreaseTransition(true, "two-step", halfwayMorphState) }, { movingStrokeScale: 1, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 1 });
  assert.deepEqual({ ...getCreaseTransition(true, "mixed", halfwayMorphState) }, { movingStrokeScale: 0.5, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 0.5 });
  assert.deepEqual({ ...getCreaseTransition(false, "mixed", halfwayMorphState) }, { movingStrokeScale: 1, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 1 });
  assert.deepEqual({ ...getCreaseTransition(false, "mixed", getCubeMorphState(1)) }, { movingStrokeScale: 0, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 0 });
  assert.match(editorSource, /id="animate-crease-edges" type="checkbox" checked/);
  for (const mode of ["fade", "shrink", "two-step", "mixed"]) {
    assert.match(editorSource, new RegExp(`<option value="${mode}"`));
  }
  assert.match(editorSource, /<option value="two-step" selected>Two step<\/option>/);
  assert.doesNotMatch(editorSource, /<option value="mixed" selected>/);
  assert.match(editorSource, /creaseTransition = getCreaseTransition\(animateCreaseEdgesInput\.checked, creaseTransitionModeInput\.value, cubeMorphState\);/);
  assert.match(editorSource, /creaseTransitionModeInput\.disabled = !animateCreaseEdgesInput\.checked;/);
});

test("final cube shading follows a direct color path throughout the morph", () => {
  assert.equal(getCubeMorphState(0).kGeometryScale, 1);
  assert.equal(getCubeMorphState(0.35).kGeometryScale, 0.5);
  assert.equal(getCubeMorphState(0.7).kGeometryScale, 0);
  assert.equal(getCubeMorphState(1).kGeometryScale, 0);
  assert.equal(getCubeMorphState(0.35).depthShadingScale, undefined);
  assert.match(editorSource, /depthShadingScale = shadeFinalCubeInput\.checked \? 1 : cubeMorphState\.kGeometryScale;/);
  assert.match(editorSource, /edgeEffects\.depthShadingAmount \*= depthShadingScale;/);
});

test("final cube shading can be disabled without disabling K depth shading", () => {
  assert.match(editorSource, /id="shade-final-cube" type="checkbox" checked/);
  assert.match(editorSource, /shadeFinalCubeInput\.disabled = transparentFacesInput\.checked \|\| !depthShadingInput\.checked;/);
  assert.match(editorSource, /shadeFinalCubeInput\.addEventListener\("change", updateCube\);/);
  assert.equal(getCubeMorphState(0).kGeometryScale, 1);
  assert.equal(getCubeMorphState(1).kGeometryScale, 0);
});

test("the final cube shades six complete faces instead of subdivided K walls", () => {
  const cubeGeometry = renderingApi.getCubeGeometry(22, 45);
  const cubePolygonPoints = renderingApi.getKPolygonPoints({ a0: 0.33, a1: 0.6, b0: 0, c0: 0.33, c1: 0.5, d0: 0, e0: 0, e1: 0.5 }, 1);
  const svgSource = renderingApi.getKPrismSvgSource(cubePolygonPoints, "left", 1, "front", cubeGeometry, false, 0.18, 1, { movingStrokeScale: 0, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 0 }, 2, "#000000", "#61adff", {
    renderId: "closed-cube-shading",
    shouldUseTransparentFaces: false,
    shouldUseDepthShading: true,
    depthShadingAmount: 0.35,
    faceDepthGradientAmount: 0.15,
    shouldUseNeonGlow: false,
    glowRadius: 6,
    glowIntensity: 0.2,
    glowColor: "#000000",
    shouldUseStrokeGradient: false,
    gradientStartColor: "#000000",
    gradientEndColor: "#000000",
    gradientAngle: 45,
  });

  assert.equal((svgSource.match(/<linearGradient id="closed-cube-shading-face-depth-gradient-/g) ?? []).length, 6);
  assert.match(editorSource, /const shouldRenderSimplifiedCube = simplifiedKPolygonPoints\.length === 4/);
  assert.match(editorSource, /const faceBoundaryPointIndices = shouldRenderSimplifiedCube \? simplifiedKPolygonPointIndices : surfaceKPolygonPoints\.map/);
  assert.match(editorSource, /if \(!shouldRenderSimplifiedCube\) \{\s+faceVertexIndices\.push/);
});

test("two-step keeps closed-cube crease edges available for its second stage", () => {
  const cubeGeometry = renderingApi.getCubeGeometry(22, 45);
  const cubePolygonPoints = renderingApi.getKPolygonPoints({ a0: 0.33, a1: 0.6, b0: 0, c0: 0.33, c1: 0.5, d0: 0, e0: 0, e1: 0.5 }, 1);
  const svgSource = renderingApi.getKPrismSvgSource(cubePolygonPoints, "left", 1, "front", cubeGeometry, false, 0.18, 1, { movingStrokeScale: 0.5, movingOpacity: 0.5, stationaryStrokeScale: 0.5, stationaryOpacity: 0.5 }, 2, "#000000", "#61adff", {
    renderId: "two-step-closed-cube",
    shouldUseTransparentFaces: false,
    shouldUseDepthShading: true,
    depthShadingAmount: 0.35,
    faceDepthGradientAmount: 0.15,
    shouldUseNeonGlow: false,
    glowRadius: 6,
    glowIntensity: 0.2,
    glowColor: "#000000",
    shouldUseStrokeGradient: false,
    gradientStartColor: "#000000",
    gradientEndColor: "#000000",
    gradientAngle: 45,
  });

  assert.match(svgSource, /<path d="M[^"]+" fill="none" stroke="#000000" stroke-width="1" stroke-linecap="butt" stroke-linejoin="round" opacity="0\.5" \/>/);
});

test("pointer cube morph uses the shared cube morph state in reverse", () => {
  assert.doesNotMatch(editorSource, /pointer-k-click|Pointer cube pulse/);
  assert.match(editorSource, /if \(pointerCubeMorphEffectNames\.has\(effectName\) && clickMorphState === undefined\) \{\s+boxProgress = 1;/);
  assert.match(editorSource, /const clickMorphProgress = clickMorphState === undefined \? 0 : getEasedEffectProgress\(clickMorphState\.progress \/ 2\);/);
  assert.match(editorSource, /if \(pointerCubeMorphEffectNames\.has\(effectName\)\) \{\s+boxProgress = 1 - clickMorphProgress;/);
});

test("every K and cube transition uses the cube morph state", () => {
  assert.match(editorSource, /if \(effectName === "cube-morph"\) \{\s+boxProgress = transitionProgress;/);
  assert.match(editorSource, /else if \(reverseCubeMorphEffectNames\.has\(effectName\) && clickMorphState === undefined\) \{\s+boxProgress = 1 - transitionProgress;/);
  assert.match(editorSource, /if \(boxProgress !== undefined\) \{\s+const cubeMorphState = getCubeMorphState\(boxProgress\);/);
  assert.match(editorSource, /parameterValues\.b0 \*= cubeMorphState\.kGeometryScale;\s+parameterValues\.d0 \*= cubeMorphState\.kGeometryScale;\s+parameterValues\.e0 \*= cubeMorphState\.kGeometryScale;/);
  assert.match(editorSource, /creaseTransition = getCreaseTransition\(animateCreaseEdgesInput\.checked, creaseTransitionModeInput\.value, cubeMorphState\);/);
  assert.doesNotMatch(editorSource, /clickGeometryStageProgress|clickSurfaceDetailStageProgress|isClickMorphReturning/);
});

test("selected-view cube reveal uses the live angle controls for its K endpoint", () => {
  assert.match(editorSource, /const selectedViewReverseCubeMorphEffectNames = new Set\(\["reverse-cube-morph-selected-view", "reverse-cube-morph-selected-view-hidden-edge-grow"\]\);/);
  assert.match(editorSource, /const reverseKEndTopAngle = selectedViewReverseCubeMorphEffectNames\.has\(effectName\) \? topAngleDegrees : EFFECT_REVERSE_K_END_TOP_ANGLE;/);
  assert.match(editorSource, /const reverseKEndHorizontalAngle = selectedViewReverseCubeMorphEffectNames\.has\(effectName\) \? horizontalAngleDegrees : EFFECT_REVERSE_K_END_HORIZONTAL_ANGLE;/);
  assert.match(editorSource, /const reverseBoxStartTopAngle = selectedViewReverseCubeMorphEffectNames\.has\(effectName\) \? reverseKEndTopAngle \+ EFFECT_REVERSE_BOX_START_TOP_ANGLE - EFFECT_REVERSE_K_END_TOP_ANGLE : EFFECT_REVERSE_BOX_START_TOP_ANGLE;/);
  assert.match(editorSource, /const reverseBoxStartHorizontalAngle = selectedViewReverseCubeMorphEffectNames\.has\(effectName\) \? reverseKEndHorizontalAngle \+ EFFECT_REVERSE_BOX_START_HORIZONTAL_ANGLE - EFFECT_REVERSE_K_END_HORIZONTAL_ANGLE : EFFECT_REVERSE_BOX_START_HORIZONTAL_ANGLE;/);
  assert.match(editorSource, /topAngleDegrees = reverseKEndTopAngle \+ \(reverseBoxStartTopAngle - reverseKEndTopAngle\) \* boxProgress;/);
  assert.match(editorSource, /horizontalAngleDegrees = reverseKEndHorizontalAngle \+ \(reverseBoxStartHorizontalAngle - reverseKEndHorizontalAngle\) \* boxProgress;/);
  assert.match(editorSource, /const reverseMorphClickEffectNames = new Set\(\[\.\.\.reverseCubeMorphEffectNames, \.\.\.pointerEffectNames\]\);/);
});

test("the fixed cube reveal ends at a 12.5 degree top view", () => {
  assert.match(editorSource, /const EFFECT_REVERSE_K_END_TOP_ANGLE = 12\.5;/);
});

test("hidden-edge cube reveal grows hidden stroke width regardless of the global checkbox", () => {
  assert.match(editorSource, /const hiddenEdgeGrowReverseCubeMorphEffectNames = new Set\(\["reverse-cube-morph-hidden-edge-grow", "reverse-cube-morph-selected-view-hidden-edge-grow"\]\);/);
  assert.match(editorSource, /const shouldShowHiddenEdges = showHiddenEdgesInput\.checked \|\| hiddenEdgeGrowReverseCubeMorphEffectNames\.has\(effectName\);/);
  assert.match(editorSource, /hiddenEdgeStrokeScale = hiddenEdgeGrowReverseCubeMorphEffectNames\.has\(effectName\) \? cubeMorphState\.kGeometryScale : 1;/);
});

test("the 22 by 45 pointer clones share pointer and click behavior around their own center", () => {
  assert.match(editorSource, /const EFFECT_POINTER_SIDE_TOP_CENTER_ANGLE = 22;/);
  assert.match(editorSource, /const EFFECT_POINTER_SIDE_HORIZONTAL_CENTER_ANGLE = 45;/);
  assert.match(editorSource, /"pointer-angle-side": sidePointerAngles/);
  assert.match(editorSource, /"pointer-box-click-side": sidePointerAngles/);
  assert.match(editorSource, /const pointerCubeMorphEffectNames = new Set\(\["pointer-box-click", "pointer-box-click-side"\]\);/);
  assert.match(editorSource, /topAngleDegrees: pointerEffectAngles\.centerTopAngle \+ \(verticalProgress - 0\.5\) \* EFFECT_POINTER_ANGLE_RANGE/);
  assert.match(editorSource, /horizontalAngleDegrees: pointerEffectAngles\.centerHorizontalAngle \+ \(horizontalProgress - 0\.5\) \* EFFECT_POINTER_ANGLE_RANGE/);
});

test("the head-on to side clone starts with a small visible angle", () => {
  assert.match(editorSource, /data-effect="angled-head-on-extrude"/);
  assert.match(editorSource, /const EFFECT_ANGLED_START_TOP_ANGLE = 8;/);
  assert.match(editorSource, /const EFFECT_ANGLED_START_HORIZONTAL_ANGLE = 78;/);
  assert.match(editorSource, /topAngleDegrees = EFFECT_ANGLED_START_TOP_ANGLE \+ \(EFFECT_SIDE_TOP_ANGLE - EFFECT_ANGLED_START_TOP_ANGLE\) \* transitionProgress;/);
  assert.match(editorSource, /horizontalAngleDegrees = EFFECT_ANGLED_START_HORIZONTAL_ANGLE \+ \(EFFECT_SIDE_HORIZONTAL_ANGLE - EFFECT_ANGLED_START_HORIZONTAL_ANGLE\) \* transitionProgress;/);
});

test("stroke grow and shrink examples move the outline in opposite directions", () => {
  assert.match(editorSource, /data-effect="stroke-pulse"/);
  assert.match(editorSource, /data-effect="stroke-shrink"/);
  assert.match(editorSource, /effectName === "stroke-pulse"\) \{\s+strokeWidth \*= 1 \+ transitionProgress \* 0\.2;/);
  assert.match(editorSource, /effectName === "stroke-shrink"\) \{\s+strokeWidth \*= 1 - transitionProgress \* 0\.2;/);
});

test("the body-diagonal spins cover all four opposite-corner axes", () => {
  for (const effectName of ["body-diagonal-spin", "body-diagonal-spin-001-110", "body-diagonal-spin-010-101", "body-diagonal-spin-011-100"]) {
    assert.match(editorSource, new RegExp(`data-effect="${effectName}"`));
  }
  assert.doesNotMatch(editorSource, /both-spin|Both-axis spin/);
  assert.doesNotMatch(editorSource, /horizontal-360-vertical-180-spin/);
  assert.match(editorSource, /const bodyDiagonalAxisOfEffectName = \{ "body-diagonal-spin": \[1, 1, 1\], "body-diagonal-spin-001-110": \[1, 1, -1\], "body-diagonal-spin-010-101": \[1, -1, 1\], "body-diagonal-spin-011-100": \[-1, 1, 1\] \};/);
  assert.match(editorSource, /getCubeGeometryWithAxisRotation\(topAngleDegrees, horizontalAngleDegrees, transitionProgress \* 360, bodyDiagonalAxis\)/);
  assert.match(editorSource, /bodyDiagonalSpinEffectNames\.has\(effectCard\.dataset\.effect\)/);
});

test("cube angle editor starts from the selected K configuration", () => {
  for (const [parameterName, parameterValue] of [["a0", "0.33"], ["a1", "0.60"], ["b0", "0.34"], ["c0", "0.33"], ["c1", "0.50"], ["d0", "0.22"], ["e0", "0.39"], ["e1", "0.50"]]) {
    assert.match(editorSource, new RegExp(`id="${parameterName}" type="range" min="0" max="1" value="${parameterValue}"`));
  }
  assert.match(editorSource, /id="parallel-top" type="checkbox" checked \/>/);
  assert.match(editorSource, /id="parallel-bottom" type="checkbox" checked/);
  assert.match(editorSource, /id="align-inlet-starts" type="checkbox" checked \/>/);
  assert.match(editorSource, /id="stroke-color" type="color" value="#002c94"/);
  assert.match(editorSource, /<option value="front" selected>Front<\/option>/);
});
