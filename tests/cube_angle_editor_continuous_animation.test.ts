import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const scriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(scriptSource);
const quaternionFunctionSource = scriptSource.match(/const getUpdatedRotationQuaternion = \(rotationQuaternion, angularVelocity, elapsedSeconds\) => \{[\s\S]*?\n      \};/)?.[0];
assert.ok(quaternionFunctionSource);
const getUpdatedRotationQuaternion = vm.runInNewContext(`${quaternionFunctionSource}\ngetUpdatedRotationQuaternion;`);

test("continuous cube rotation composes small quaternion steps without orientation drift", () => {
  const firstHalf = getUpdatedRotationQuaternion([0, 0, 0, 1], [0, 90, 0], 0.5);
  const secondHalf = getUpdatedRotationQuaternion(firstHalf, [0, 90, 0], 0.5);
  const fullStep = getUpdatedRotationQuaternion([0, 0, 0, 1], [0, 90, 0], 1);

  for (let componentIndex = 0; componentIndex < 4; componentIndex += 1) {
    assert.ok(Math.abs(secondHalf[componentIndex] - fullStep[componentIndex]) < 0.000000000001);
  }
  assert.ok(Math.abs(Math.hypot(...secondHalf) - 1) < 0.000000000001);
});

test("the continuous cards render closed cubes and keep the shared animation frame alive", () => {
  assert.match(editorSource, /data-effect="ambient-cube-spin"/);
  assert.match(editorSource, /data-effect="inertial-cube-wander"/);
  assert.match(editorSource, /data-effect="inertial-cube-wander-fast"/);
  assert.match(editorSource, /const continuousEffectNames = new Set\(\["ambient-cube-spin", \.\.\.inertialEffectNames\]\);/);
  assert.match(editorSource, /if \(continuousRotationState !== undefined\) \{\s+boxProgress = 1;/);
  assert.match(editorSource, /continuousEffectCards\.length > 0 \|\| transitioningEffectCards\.size > 0 \|\| clickMorphEffectCards\.size > 0/);
});

test("inertial wandering eases velocity toward fresh targets lasting two to ten seconds", () => {
  assert.match(editorSource, /const EFFECT_CONTINUOUS_TARGET_MIN_MILLISECONDS = 2000;/);
  assert.match(editorSource, /const EFFECT_CONTINUOUS_TARGET_MAX_MILLISECONDS = 10000;/);
  assert.match(editorSource, /continuousRotationState\.targetAngularVelocity = getRandomAngularVelocity\(\.\.\.inertialSpeedRange, projectionAxisOfName, minimumCameraUpComponent\);/);
  assert.match(editorSource, /continuousRotationState\.targetChangeTime = currentTime \+ EFFECT_CONTINUOUS_TARGET_MIN_MILLISECONDS \+ Math\.random\(\) \* \(EFFECT_CONTINUOUS_TARGET_MAX_MILLISECONDS - EFFECT_CONTINUOUS_TARGET_MIN_MILLISECONDS\);/);
  assert.match(editorSource, /const velocityBlend = 1 - Math\.exp\(-elapsedSeconds \/ EFFECT_INERTIAL_RESPONSE_SECONDS\);/);
  assert.match(editorSource, /continuousRotationState\.angularVelocity\[axisIndex\] \+= \(continuousRotationState\.targetAngularVelocity\[axisIndex\] - continuousRotationState\.angularVelocity\[axisIndex\]\) \* velocityBlend;/);
  assert.match(editorSource, /continuousRotationState\.rotationQuaternion = getUpdatedRotationQuaternion\(continuousRotationState\.rotationQuaternion, continuousRotationState\.angularVelocity, elapsedSeconds\);/);
  assert.doesNotMatch(editorSource, /continuousRotationState\.rotationQuaternion = \[0, 0, 0, 1\];/);
});

test("random angular targets stay in the rightward rotation hemisphere", () => {
  const speedConstantsSource = scriptSource.match(/const EFFECT_INERTIAL_MIN_SPEED = 18;[\s\S]*?const EFFECT_INERTIAL_MIN_RIGHTWARD_COMPONENT = 0\.8;/)?.[0];
  const projectionFunctionSource = scriptSource.match(/const getCubeProjectionAxes = \(topAngleDegrees, horizontalAngleDegrees\) => \{[\s\S]*?\n      \};/)?.[0];
  const randomVelocityFunctionSource = scriptSource.match(/const getRandomAngularVelocity = \(minimumSpeed, maximumSpeed, projectionAxisOfName, minimumCameraUpComponent\) => \{[\s\S]*?\n      \};/)?.[0];
  assert.ok(speedConstantsSource);
  assert.ok(projectionFunctionSource);
  assert.ok(randomVelocityFunctionSource);
  const randomValues = [0, 0.25, 0.5, 0.75, 0.999999];
  for (const rightwardRandomValue of randomValues) {
    const testMath = Object.create(Math);
    const values = [rightwardRandomValue, 0.25, 0.5];
    testMath.random = () => values.shift();
    const result = vm.runInNewContext(`${speedConstantsSource}\n${projectionFunctionSource}\n${randomVelocityFunctionSource}\nconst projectionAxisOfName = getCubeProjectionAxes(68, 44); const angularVelocity = getRandomAngularVelocity(EFFECT_INERTIAL_MIN_SPEED, EFFECT_INERTIAL_MAX_SPEED, projectionAxisOfName, EFFECT_INERTIAL_MIN_RIGHTWARD_COMPONENT); ({ angularVelocity, projectionAxisOfName });`, { Math: testMath });
    const angularSpeed = Math.hypot(...result.angularVelocity);
    const rightwardSpeed = result.angularVelocity[0] * result.projectionAxisOfName.screenY.x + result.angularVelocity[1] * result.projectionAxisOfName.screenY.y + result.angularVelocity[2] * result.projectionAxisOfName.screenY.z;
    assert.ok(rightwardSpeed / angularSpeed >= 0.8 - 0.000000000001);
    assert.ok(angularSpeed >= 18 && angularSpeed <= 55);
  }
});

test("the fast inertial clone uses a higher speed range without changing its motion model", () => {
  assert.match(editorSource, /const EFFECT_INERTIAL_FAST_MIN_SPEED = 40;/);
  assert.match(editorSource, /const EFFECT_INERTIAL_FAST_MAX_SPEED = 95;/);
  assert.match(editorSource, /"inertial-cube-wander-fast": \[EFFECT_INERTIAL_FAST_MIN_SPEED, EFFECT_INERTIAL_FAST_MAX_SPEED\]/);
  assert.match(editorSource, /const inertialSpeedRange = inertialSpeedRangeOfEffectName\[effectCard\.dataset\.effect\];/);
});

test("the unrestricted inertial copies sample the full rotation sphere at both speed ranges", () => {
  assert.match(editorSource, /data-effect="inertial-cube-wander-free"/);
  assert.match(editorSource, /data-effect="inertial-cube-wander-fast-free"/);
  assert.match(editorSource, /"inertial-cube-wander-free": \[EFFECT_INERTIAL_MIN_SPEED, EFFECT_INERTIAL_MAX_SPEED\]/);
  assert.match(editorSource, /"inertial-cube-wander-fast-free": \[EFFECT_INERTIAL_FAST_MIN_SPEED, EFFECT_INERTIAL_FAST_MAX_SPEED\]/);
  assert.match(editorSource, /const unrestrictedInertialEffectNames = new Set\(\["inertial-cube-wander-free", "inertial-cube-wander-fast-free"\]\);/);
  assert.match(editorSource, /const minimumCameraUpComponent = unrestrictedInertialEffectNames\.has\(effectCard\.dataset\.effect\) \? -1 : EFFECT_INERTIAL_MIN_RIGHTWARD_COMPONENT;/);
});
