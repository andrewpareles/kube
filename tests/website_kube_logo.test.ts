import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

// The website draws its logo with a copy of the generator that lives in the cube angle editor.
// These tests exist mainly to catch the two copies drifting apart, which would silently make the
// shipped mark stop matching what the editor previews.

const {
  cubeView,
  foldHoverCycle,
  headerHoverCycle,
  restingKView,
  headerTumbleCycle,
  fastWanderSpeed,
  renderKubeLogoSvg,
  getEasedProgress,
  getLevelLegsFrame,
  getLevelLegsRollDegrees,
  getRestingMarkBounds,
  renderRestingKubeLogoSvg,
  restingLogoPreset,
  createWanderState,
  advanceWanderState,
} = await import("../packages/website/lib/kubeLogo/kubeLogoRender.ts");
const { VIEW_BOX_SIZE } = await import("../packages/website/lib/kubeLogo/cubeGeometry.js");

const websiteGeometrySource = fs.readFileSync(repoPath("packages/website/lib/kubeLogo/cubeGeometry.js"), "utf8");
const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const editorScriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(editorScriptSource);

// Both copies are evaluated the same way so their output can be compared directly.
const buildApi = (source: string) => vm.runInNewContext(
  `${source}\n({ getCubeGeometry, getKPolygonPoints, getKPrismSvgSource, getCubeMorphState, getCreaseTransition });`,
);

const editorConstantsSource = editorScriptSource.match(/const VIEW_BOX_SIZE = 256;[\s\S]*?const HIDDEN_EDGE_OPACITY = 0\.18;/)?.[0];
const editorRenderingSource = editorScriptSource.match(/const roundCoordinate = [\s\S]*?(?=\n      \/\/ -------------------------- Hover studies)/)?.[0];
const editorMorphSource = editorScriptSource.match(/const EFFECT_MORPH_GEOMETRY_END = .*?;/)?.[0];
const editorMorphStateSource = editorScriptSource.match(/const getCubeMorphState = \(boxProgress\) => \(\{[\s\S]*?\n {6}\}\);/)?.[0];
const editorCreaseSource = editorScriptSource.match(/const getCreaseTransition = \(shouldAnimate, transitionMode, cubeMorphState\) => \{[\s\S]*?\n {6}\};/)?.[0];
assert.ok(editorConstantsSource);
assert.ok(editorRenderingSource);
assert.ok(editorMorphSource);
assert.ok(editorMorphStateSource);
assert.ok(editorCreaseSource);

const editorApi = buildApi([
  editorConstantsSource,
  editorRenderingSource,
  editorMorphSource,
  editorMorphStateSource,
  editorCreaseSource,
].join("\n"));

// The website copy is a module, so its export block is dropped to evaluate it the same way.
const websiteApi = buildApi(websiteGeometrySource.replace(/\nexport \{[\s\S]*?\};\n/, "\n"));

const K_SHAPE = { a0: 0.33, a1: 0.6, b0: 0.34, c0: 0.33, c1: 0.5, d0: 0.22, e0: 0.39, e1: 0.5 };

test("the website generator projects the cube exactly like the editor", () => {
  for (const [topAngle, horizontalAngle] of [[5, 80], [22, 51], [22, 45], [20, 110], [0, 0], [90, 180]]) {
    const fromEditor = editorApi.getCubeGeometry(topAngle, horizontalAngle);
    const fromWebsite = websiteApi.getCubeGeometry(topAngle, horizontalAngle);
    const cornerKeys = Object.keys(fromEditor.vertexOfKey);

    // The two copies are evaluated in separate realms, so their corners are compared by value
    // rather than with deepEqual, which would trip over the mismatched prototypes.
    assert.deepEqual(Object.keys(fromWebsite.vertexOfKey).sort(), cornerKeys.sort());
    for (const cornerKey of cornerKeys) {
      const [editorX, editorY] = fromEditor.vertexOfKey[cornerKey];
      const [websiteX, websiteY] = fromWebsite.vertexOfKey[cornerKey];

      assert.equal(websiteX, editorX, `corner ${cornerKey} x differs at ${topAngle}/${horizontalAngle}`);
      assert.equal(websiteY, editorY, `corner ${cornerKey} y differs at ${topAngle}/${horizontalAngle}`);
    }
  }
});

test("the website generator draws the same K prism as the editor across the whole morph", () => {
  const creaseTransition = { movingStrokeScale: 1, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 1 };
  const edgeEffects = {
    renderId: "drift-check",
    shouldUseTransparentFaces: false,
    shouldUseDepthShading: true,
    depthShadingAmount: 0.35,
    faceDepthGradientAmount: 0.15,
    shouldUseNeonGlow: true,
    glowRadius: 1.5,
    glowIntensity: 0.25,
    glowColor: "#336dff",
    shouldUseStrokeGradient: true,
    gradientStartColor: "#4c00ff",
    gradientEndColor: "#33a7ff",
    gradientAngle: 42,
  };

  for (const boxProgress of [0, 0.25, 0.5, 0.75, 1]) {
    const renderWith = (api: ReturnType<typeof buildApi>) => {
      const cubeMorphState = api.getCubeMorphState(boxProgress);
      const kPolygonPoints = api.getKPolygonPoints({
        ...K_SHAPE,
        b0: K_SHAPE.b0 * cubeMorphState.kGeometryScale,
        d0: K_SHAPE.d0 * cubeMorphState.kGeometryScale,
        e0: K_SHAPE.e0 * cubeMorphState.kGeometryScale,
      }, 1 - cubeMorphState.kGeometryScale);

      return api.getKPrismSvgSource(
        kPolygonPoints, "left", 1, "front", api.getCubeGeometry(22, 51),
        true, 0.18, 1, api.getCreaseTransition(true, "two-step", cubeMorphState),
        13, "#4c00ff", "#f4f0ff", edgeEffects,
      );
    };

    assert.equal(renderWith(websiteApi), renderWith(editorApi), `prism differs at boxProgress ${boxProgress}`);
  }
});

test("both marks rest at one shared angle and land the cube at another", () => {
  assert.deepEqual(restingKView, { topAngle: 5, horizontalAngle: 80 });
  assert.deepEqual(cubeView, { topAngle: 22, horizontalAngle: 45 });
  assert.notDeepEqual(restingKView, cubeView);
});

test("every hover effect draws something different from resting", () => {
  const renderAt = (effectIndex: number, progress: number, wanderState: ReturnType<typeof createWanderState>) => renderKubeLogoSvg({
    frame: headerHoverCycle[effectIndex].frameAt(getEasedProgress(progress), { baseView: restingKView, wanderState }),
    renderId: "cycle-check",
    sidePx: 34,
    strokeScale: 1.625,
  });

  // A drift that has not been anywhere is the resting mark, whichever effect is asked for it: the
  // orientation is the drift's own, so a fresh one is square on.
  const restingSvgOfIndex = headerHoverCycle.map((_, effectIndex) => renderAt(effectIndex, 0, createWanderState()));
  for (const restingSvg of restingSvgOfIndex) {
    assert.equal(restingSvg, restingSvgOfIndex[0], "every effect must start from the same resting mark");
  }

  // The tumbling effects need a turned drift before they differ from the resting mark.
  const turnedWanderState = createWanderState();
  advanceWanderState({ wanderState: turnedWanderState, currentTimeMs: 0, elapsedSeconds: 0.5, speedRange: fastWanderSpeed, isGoingHome: false });

  // Sampled part way through rather than at the end, because the three spins turn a full circle
  // and so finish back exactly where they started.
  for (const [effectIndex, hoverEffect] of headerHoverCycle.entries()) {
    assert.notEqual(renderAt(effectIndex, 0.4, turnedWanderState), restingSvgOfIndex[0], `effect "${hoverEffect.label}" never moves`);
  }
});

test("a tumble picked back up part way home carries on from where it is", () => {
  const tumble = (wanderState: ReturnType<typeof createWanderState>, isGoingHome: boolean, frameCount: number) => {
    for (let frame = 0; frame < frameCount; frame += 1) {
      advanceWanderState({ wanderState, currentTimeMs: frame * 16, elapsedSeconds: 0.016, speedRange: fastWanderSpeed, isGoingHome });
    }
  };
  const turnBetween = (fromQuaternion: number[], toQuaternion: number[]) => {
    const dot = Math.abs(fromQuaternion.reduce((total, component, index) => total + component * toQuaternion[index], 0));
    return 2 * Math.acos(Math.min(1, dot)) * 180 / Math.PI;
  };

  const wanderState = createWanderState();
  tumble(wanderState, false, 60);
  tumble(wanderState, true, 20);
  const whereItWasCaught = wanderState.rotationQuaternion;

  // Hovering again must not jump the mark anywhere: the very next frame carries on from the angle
  // it was caught at, rather than snapping back onto an arc worked out from the hover.
  const whereItWasHeadingBefore = wanderState.targetChangeTimeMs;
  tumble(wanderState, false, 1);
  assert.ok(
    turnBetween(whereItWasCaught, wanderState.rotationQuaternion) < 3,
    `picking the tumble back up jumped it ${turnBetween(whereItWasCaught, wanderState.rotationQuaternion).toFixed(1)} degrees`,
  );

  // It should be heading somewhere new, though, rather than finishing the trip it was on.
  assert.notEqual(wanderState.targetChangeTimeMs, whereItWasHeadingBefore);

  // And left alone it still comes all the way back to square rather than resting crooked.
  tumble(wanderState, true, 80);
  assert.equal(wanderState.returnProgress, 1);
  assert.ok(turnBetween(wanderState.rotationQuaternion, [0, 0, 0, 1]) < 0.001, "the mark must come back square");
});

test("the two spins and the diagonal spin pass through different orientations", () => {
  const wanderState = createWanderState();
  const spinLabels = ["Horizontal spin", "Vertical spin", "Body-diagonal spin"];
  const midFlightSvgs = spinLabels.map((spinLabel) => {
    const effect = headerHoverCycle.find((hoverEffect) => hoverEffect.label === spinLabel);
    assert.ok(effect, `missing effect ${spinLabel}`);

    return renderKubeLogoSvg({
      frame: effect.frameAt(0.4, { baseView: restingKView, wanderState }),
      renderId: "spin-check",
      sidePx: 34,
      strokeScale: 1.625,
    });
  });

  assert.equal(new Set(midFlightSvgs).size, spinLabels.length, "spins must not share an orientation mid-flight");
});

test("a spin returns to exactly the resting mark", () => {
  const wanderState = createWanderState();
  const spinEffect = headerHoverCycle.find((hoverEffect) => hoverEffect.label === "Body-diagonal spin");
  assert.ok(spinEffect);

  const render = (frame: ReturnType<typeof spinEffect.frameAt>) => renderKubeLogoSvg({
    frame, renderId: "return-check", sidePx: 34, strokeScale: 1.625,
  });

  const cubeEffect = headerHoverCycle[0];
  assert.equal(
    render(spinEffect.frameAt(0, { baseView: restingKView, wanderState })),
    render(cubeEffect.frameAt(0, { baseView: restingKView, wanderState })),
  );
});

test("the header tumbles the K itself, fading its hidden edges in without closing it", () => {
  assert.equal(headerTumbleCycle.length, 1);
  const [tumbleEffect] = headerTumbleCycle;
  assert.deepEqual(tumbleEffect.wanderSpeedRange, fastWanderSpeed);

  const wanderState = createWanderState();
  advanceWanderState({ wanderState, currentTimeMs: 0, elapsedSeconds: 0.5, speedRange: fastWanderSpeed, isGoingHome: false });

  const renderAt = (progress: number) => renderKubeLogoSvg({
    frame: tumbleEffect.frameAt(getEasedProgress(progress), { baseView: restingKView, wanderState }),
    renderId: "tumble-check",
    sidePx: 34,
    strokeScale: 1.625,
  });

  // Everything the generator draws is fully opaque except the hidden edges, so the one opacity
  // below 1 is theirs. The leading space keeps this off flood-opacity, which belongs to the glow.
  const getHiddenEdgeOpacity = (svgSource: string) => {
    const partialOpacities = [...svgSource.matchAll(/\sopacity="([\d.]+)"/g)]
      .map((opacityMatch) => Number(opacityMatch[1]))
      .filter((opacity) => opacity < 1);

    return partialOpacities.length === 0 ? 0 : Math.max(...partialOpacities);
  };

  assert.equal(getHiddenEdgeOpacity(renderAt(0)), 0, "resting must not show hidden edges");
  assert.ok(getHiddenEdgeOpacity(renderAt(0.5)) > 0, "hidden edges must fade in part way through");
  assert.ok(
    getHiddenEdgeOpacity(renderAt(1)) > getHiddenEdgeOpacity(renderAt(0.5)),
    "hidden edges must keep strengthening as the hover plays",
  );

  // The K must stay a K. Asserted on the frame rather than on how many paths get drawn, because
  // the visible face count changes with whichever way the tumble happens to be facing.
  const playedFrame = tumbleEffect.frameAt(1, { baseView: restingKView, wanderState });
  assert.equal(playedFrame.boxProgress, 0, "the tumbling mark must not close into a cube");
});

test("the header uses the fold effect and starts at rest", () => {
  const createPageSource = fs.readFileSync(repoPath("packages/website/app/create/CreatePage.tsx"), "utf8");
  const headerSource = fs.readFileSync(repoPath("packages/website/components/site/SiteBrandLink.tsx"), "utf8");

  // The opening move runs the fold's own cycle entry backwards, so its first frame has to be
  // the closed cube rather than the resting K.
  assert.equal(createPageSource.includes("<KubeLogoMark"), false);
  assert.match(headerSource, /hoverCycle=\{foldHoverCycle\}/);
  assert.match(headerSource, /introMilliseconds=\{null\}/);

  const countPaths = (svgSource: string) => (svgSource.match(/<path /g) ?? []).length;
  const renderAt = (progress: number) => renderKubeLogoSvg({
    frame: foldHoverCycle[0].frameAt(progress, { baseView: restingKView, wanderState: createWanderState() }),
    renderId: "intro-check",
    sidePx: 196,
    strokeScale: 1,
  });

  assert.ok(countPaths(renderAt(1)) < countPaths(renderAt(0)), "the opening frame must be the simpler closed cube");
});

test("the mark measures its own drawing, so nothing hovers the empty square around it", () => {
  const strokeScale = 1;
  // The bounds move with the stroke, so they are measured in the same look they are drawn in.
  const markBounds = getRestingMarkBounds({ view: restingKView, strokeScale, isKLevel: false, preset: restingLogoPreset });

  // A K standing on one face of the cube leaves most of the square empty. Getting this wrong in
  // either direction is what the numbers below are for: too generous and the mark answers to the
  // pointer well away from the drawing, too tight and part of the K stops answering at all.
  assert.ok(markBounds.width < 0.7, `the mark claims ${markBounds.width} of the square across`);
  assert.ok(markBounds.height < 0.7, `the mark claims ${markBounds.height} of the square down`);
  assert.ok(markBounds.width > 0.4 && markBounds.height > 0.4, "the mark cannot have shrunk to nothing");

  // Whatever it claims has to hold every line it draws.
  const svgSource = renderRestingKubeLogoSvg({
    view: restingKView, renderId: "bounds-check", sidePx: VIEW_BOX_SIZE, strokeScale, isKLevel: false, preset: restingLogoPreset,
  });
  const drawnCoordinates = [...svgSource.matchAll(/ d="([^"]*)"/g)]
    .flatMap((pathMatch) => pathMatch[1].match(/-?[\d.]+/g) ?? [])
    .map(Number);
  assert.ok(drawnCoordinates.length > 20, "no path data was found to measure");

  for (const [coordinateIndex, coordinate] of drawnCoordinates.entries()) {
    const isAcross = coordinateIndex % 2 === 0;
    const start = (isAcross ? markBounds.left : markBounds.top) * VIEW_BOX_SIZE;
    const end = start + (isAcross ? markBounds.width : markBounds.height) * VIEW_BOX_SIZE;

    assert.ok(coordinate >= start && coordinate <= end, `${coordinate} falls outside ${start}..${end}`);
  }
});

test("the pointer is answered outside the mark without the mark taking up any more room", () => {
  const kubeLogoMarkSource = fs.readFileSync(repoPath("packages/website/components/site/KubeLogoMark.tsx"), "utf8");
  const siteHeaderSource = fs.readFileSync(repoPath("packages/website/components/site/SiteBrandLink.tsx"), "utf8");
  const createPageSource = fs.readFileSync(repoPath("packages/website/app/create/CreatePage.tsx"), "utf8");

  assert.match(kubeLogoMarkSource, /hoverAreaOverhangPx: number/);
  assert.match(kubeLogoMarkSource, /<span className="absolute" style=\{\{ inset: -hoverAreaOverhangPx \}\} \/>/);
  // The reach is positioned, so what the page measures is still only the drawing itself.
  assert.match(kubeLogoMarkSource, /style=\{\{ width: sidePx \* markBounds\.width, height: sidePx \* markBounds\.height \}\}/);
  // And it sits inside the span that listens, so a link around the mark picks up presses on it.
  assert.ok(
    kubeLogoMarkSource.indexOf("inset: -hoverAreaOverhangPx") > kubeLogoMarkSource.indexOf("ref={hoverAreaRef}"),
    "the reach has to sit inside the span that listens for the pointer",
  );

  // Each mark picks its own reach. The header's is the mark itself and nothing more, because it
  // sits inches from the tabs; the one on the front page has the room to be forgiving.
  assert.match(siteHeaderSource, /hoverAreaOverhangPx=\{0\}/);
  assert.equal(createPageSource.includes("<KubeLogoMark"), false);
  assert.match(siteHeaderSource, /data-kube-logo-hover/);
  assert.match(kubeLogoMarkSource, /closest<HTMLElement>\('\[data-kube-logo-hover\]'\)/);
  assert.doesNotMatch(kubeLogoMarkSource, /addEventListener\('focusin'/);
  assert.doesNotMatch(kubeLogoMarkSource, /:focus-within/);
});

// -------------------------- Levelling the K --------------------------

// The angles of every long, nearly horizontal line along the bottom of the mark, which is where
// the K's two legs end.
const getLegBottomAngles = (svgSource: string) => [...svgSource.matchAll(/M(-?[\d.]+) (-?[\d.]+)L(-?[\d.]+) (-?[\d.]+)/g)]
  .map((segmentMatch) => segmentMatch.slice(1).map(Number))
  .map(([startX, startY, endX, endY]) => ({
    // Measured as a slope rather than a direction, so an edge drawn right to left reads the same
    // as one drawn left to right.
    slopeDegrees: Math.atan((endY - startY) / (endX - startX)) * 180 / Math.PI,
    length: Math.hypot(endX - startX, endY - startY),
    lowestPoint: Math.max(startY, endY),
  }))
  .filter((segment) => Math.abs(segment.slopeDegrees) < 10 && segment.length > 20 && segment.lowestPoint > 180)
  .map((segment) => segment.slopeDegrees);

const renderRestingFoldMark = (levelAmount: number | null) => {
  const frame = foldHoverCycle[0].frameAt(0, { baseView: restingKView, wanderState: createWanderState() });

  return renderKubeLogoSvg({
    frame: levelAmount === null ? frame : getLevelLegsFrame(frame, levelAmount),
    renderId: "level-check",
    sidePx: 196,
    strokeScale: 1,
  });
};

test("levelling the K lands the bottom of its legs flat, and leaving it alone does not", () => {
  const plainAngles = getLegBottomAngles(renderRestingFoldMark(null));
  const levelledAngles = getLegBottomAngles(renderRestingFoldMark(1));

  // Both legs end along the bottom, so anything less than two means the edges stopped being found
  // and the rest of this test would pass without measuring anything.
  assert.equal(plainAngles.length, 2);
  assert.equal(levelledAngles.length, 2);

  for (const angle of plainAngles) {
    assert.ok(Math.abs(angle) > 0.5, `the plain K's legs should still slope, got ${angle} degrees`);
  }
  for (const angle of levelledAngles) {
    assert.ok(Math.abs(angle) < 0.0001, `the levelled K's legs should be flat, got ${angle} degrees`);
  }

  // The correction is small, and it is worked out from the camera rather than typed in, so this
  // pins the number the two views actually need.
  assert.ok(Math.abs(getLevelLegsRollDegrees(restingKView) + 0.8804) < 0.0001);
  assert.ok(Math.abs(getLevelLegsRollDegrees(cubeView)) > 8);
});

test("levelling turns the shape rather than the finished drawing", () => {
  const getStrokeGradient = (svgSource: string) => svgSource.match(/<linearGradient id="level-check-stroke-gradient"[^>]*>/)?.[0];

  // Rotating the drawing would drag the gradient round with it, so the mark's lighting would tilt
  // along with its legs. Turning the shape around the line the camera looks down leaves it put.
  assert.ok(getStrokeGradient(renderRestingFoldMark(1)));
  assert.equal(getStrokeGradient(renderRestingFoldMark(1)), getStrokeGradient(renderRestingFoldMark(null)));
});

test("levelling has completely unwound by the time a hover has played", () => {
  const wanderState = createWanderState();
  advanceWanderState({ wanderState, currentTimeMs: 0, elapsedSeconds: 0.5, speedRange: fastWanderSpeed, isGoingHome: false });

  // Every hover is drawn with the levelling faded out in step with it, so whatever the mark
  // arrives at is exactly what it would have arrived at with the whole thing switched off.
  for (const hoverEffect of [foldHoverCycle[0], headerTumbleCycle[0], ...headerHoverCycle]) {
    const playedFrame = hoverEffect.frameAt(1, { baseView: restingKView, wanderState });
    const render = (frame: typeof playedFrame) => renderKubeLogoSvg({
      frame, renderId: "unwind-check", sidePx: 196, strokeScale: 1,
    });

    assert.equal(
      render(getLevelLegsFrame(playedFrame, 0)),
      render(playedFrame),
      `"${hoverEffect.label}" must finish where it would have without levelling`,
    );
  }
});

test("the fold only closes into a cube, and closing removes the K cuts", () => {
  assert.equal(foldHoverCycle.length, 1);
  assert.equal(foldHoverCycle[0].label, "Cube");

  const countPaths = (svgSource: string) => (svgSource.match(/<path /g) ?? []).length;
  const renderAt = (progress: number) => renderKubeLogoSvg({
    frame: foldHoverCycle[0].frameAt(getEasedProgress(progress), { baseView: restingKView, wanderState: createWanderState() }),
    renderId: "fold-check",
    sidePx: 196,
    strokeScale: 1,
  });

  assert.ok(countPaths(renderAt(0)) > countPaths(renderAt(1)), "the closed cube must be simpler than the open K");
});
