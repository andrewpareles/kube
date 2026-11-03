import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { repoPath } from "./testPaths.ts";

const editorSource = fs.readFileSync(repoPath("assets/app-icons/cube-angle-editor/index.html"), "utf8");
const scriptSource = editorSource.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(scriptSource);
const projectionConstantsSource = scriptSource.match(/const VIEW_BOX_SIZE = 256;[\s\S]*?const CUBE_SCALE = [^;]+;/)?.[0];
const projectionFunctionsSource = scriptSource.match(/const roundCoordinate = [\s\S]*?(?=\n      const formatPoint)/)?.[0];
assert.ok(projectionConstantsSource);
assert.ok(projectionFunctionsSource);
const projectionApi = vm.runInNewContext(`${projectionConstantsSource}\n${projectionFunctionsSource}\n({ getCubeGeometry, getCubeGeometryWithAxisRotation });`);

const assertPointAlmostEqual = (pointA: number[], pointB: number[]) => {
  assert.ok(Math.abs(pointA[0] - pointB[0]) < 0.000000001);
  assert.ok(Math.abs(pointA[1] - pointB[1]) < 0.000000001);
};

const diagonalCases = [
  { axis: [1, 1, 1], corners: ["000", "111"] },
  { axis: [1, 1, -1], corners: ["001", "110"] },
  { axis: [1, -1, 1], corners: ["010", "101"] },
  { axis: [-1, 1, 1], corners: ["011", "100"] },
];

test("a full rotation around every body diagonal returns each projected cube vertex to its start", () => {
  for (const { axis } of diagonalCases) {
    const startGeometry = projectionApi.getCubeGeometryWithAxisRotation(22, 45, 0, axis);
    const endGeometry = projectionApi.getCubeGeometryWithAxisRotation(22, 45, 360, axis);
    for (const vertexKey of Object.keys(startGeometry.vertexOfKey)) {
      assertPointAlmostEqual(startGeometry.vertexOfKey[vertexKey], endGeometry.vertexOfKey[vertexKey]);
    }
  }
});

test("each opposite-corner body diagonal remains fixed throughout its rotation", () => {
  const baseGeometry = projectionApi.getCubeGeometry(22, 45);
  for (const { axis, corners } of diagonalCases) {
    for (const rotationDegrees of [45, 120, 225, 359]) {
      const rotatedGeometry = projectionApi.getCubeGeometryWithAxisRotation(22, 45, rotationDegrees, axis);
      assertPointAlmostEqual(rotatedGeometry.vertexOfKey[corners[0]], baseGeometry.vertexOfKey[corners[0]]);
      assertPointAlmostEqual(rotatedGeometry.vertexOfKey[corners[1]], baseGeometry.vertexOfKey[corners[1]]);
    }
  }
});
