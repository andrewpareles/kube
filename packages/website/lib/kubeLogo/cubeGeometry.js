// The cube-K geometry generator, ported verbatim from the design tool at
// assets/app-icons/cube-angle-editor/index.html so the website renders exactly what the
// editor previews. Keep the two in sync: if you change the shape or projection maths here,
// make the same change there, and vice versa.
//
// This stays plain JavaScript so it can be diffed line-for-line against the editor script.
// The typed surface lives in cubeGeometry.d.ts, and kubeLogoRender.ts is the API the app uses.

const VIEW_BOX_SIZE = 256;
const CUBE_CENTER = VIEW_BOX_SIZE / 2;
const CUBE_MAX_WIDTH = 176;
const CUBE_MAX_HEIGHT = 220;
const CUBE_SCALE = Math.min(CUBE_MAX_WIDTH / Math.sqrt(2), CUBE_MAX_HEIGHT / Math.sqrt(3));
// TODO: AI-PICKED-VALUE: A one-unit matching stroke closes browser antialiasing seams between adjacent filled faces and stays beneath the main outline.
const FACE_SEAM_STROKE_WIDTH = 1;
// TODO: AI-PICKED-VALUE: This cutoff removes collapsed faces below one hundredth of a screen pixel before they can destabilize occlusion tests.
const K_POLYGON_SIMPLIFY_EPSILON = 0.00001;
// TODO: AI-PICKED-VALUE: These opacities separate construction geometry from the edges intended for the logo.
const CUBE_GUIDE_OPACITY = 0.14;
// TODO: AI-PICKED-VALUE: Eighteen percent keeps hidden edges readable without letting them compete with the visible outline.
const HIDDEN_EDGE_OPACITY = 0.18;
const fullCreaseTransition = { movingStrokeScale: 1, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: 1 };

// TODO: AI-PICKED-VALUE: Three decimal places keep rotation smooth while keeping exported SVG path data compact.
const roundCoordinate = (coordinate) => Math.round(coordinate * 1000) / 1000;

// All eight vertices are projected so horizontal rotation works continuously through every quadrant.
const getCubeGeometryFromProjectionAxes = (screenXOfAxis, screenYOfAxis, depthOfAxis) => {
  const rawVertexOfKey = {};

  for (const x of [0, 1]) {
    for (const y of [0, 1]) {
      for (const z of [0, 1]) {
        rawVertexOfKey[`${x}${y}${z}`] = [x * screenXOfAxis.x + y * screenXOfAxis.y + z * screenXOfAxis.z, x * screenYOfAxis.x + y * screenYOfAxis.y + z * screenYOfAxis.z];
      }
    }
  }

  const rawCenterX = (rawVertexOfKey["000"][0] + rawVertexOfKey["111"][0]) / 2;
  const rawCenterY = (rawVertexOfKey["000"][1] + rawVertexOfKey["111"][1]) / 2;
  const vertexOfKey = {};

  for (const [vertexKey, [rawX, rawY]] of Object.entries(rawVertexOfKey)) {
    vertexOfKey[vertexKey] = [CUBE_CENTER + (rawX - rawCenterX) * CUBE_SCALE, CUBE_CENTER + (rawY - rawCenterY) * CUBE_SCALE];
  }

  return {
    top: vertexOfKey["000"],
    rightUpper: vertexOfKey["100"],
    rightLower: vertexOfKey["110"],
    bottom: vertexOfKey["111"],
    leftLower: vertexOfKey["011"],
    leftUpper: vertexOfKey["001"],
    center: vertexOfKey["101"],
    depthOfAxis,
    cubeScale: CUBE_SCALE,
    rawCenterX,
    rawCenterY,
    screenXOfAxis,
    screenYOfAxis,
    vertexOfKey,
  };
};

const getCubeProjectionAxes = (topAngleDegrees, horizontalAngleDegrees) => {
  const topAngleRadians = topAngleDegrees * Math.PI / 180;
  const horizontalAngleRadians = horizontalAngleDegrees * Math.PI / 180;
  const topAngleSin = Math.sin(topAngleRadians);
  const topAngleCos = Math.cos(topAngleRadians);
  const horizontalAngleSin = Math.sin(horizontalAngleRadians);
  const horizontalAngleCos = Math.cos(horizontalAngleRadians);

  return {
    depth: { x: topAngleCos * horizontalAngleCos, y: -topAngleSin, z: topAngleCos * horizontalAngleSin },
    screenX: { x: horizontalAngleSin, y: 0, z: -horizontalAngleCos },
    screenY: { x: topAngleSin * horizontalAngleCos, y: topAngleCos, z: topAngleSin * horizontalAngleSin },
  };
};

const getCubeGeometry = (topAngleDegrees, horizontalAngleDegrees) => {
  const projectionAxisOfName = getCubeProjectionAxes(topAngleDegrees, horizontalAngleDegrees);
  return getCubeGeometryFromProjectionAxes(projectionAxisOfName.screenX, projectionAxisOfName.screenY, projectionAxisOfName.depth);
};

// Continuous cube effects keep one quaternion orientation so changes in angular velocity never reset or snap the object.
const getCubeGeometryWithQuaternionRotation = (topAngleDegrees, horizontalAngleDegrees, rotationQuaternion) => {
  const projectionAxisOfName = getCubeProjectionAxes(topAngleDegrees, horizontalAngleDegrees);
  const [rotationX, rotationY, rotationZ, rotationW] = rotationQuaternion;
  const rotateVector = ([x, y, z]) => {
    const quaternionVectorCrossX = rotationY * z - rotationZ * y;
    const quaternionVectorCrossY = rotationZ * x - rotationX * z;
    const quaternionVectorCrossZ = rotationX * y - rotationY * x;
    const secondCrossX = rotationY * quaternionVectorCrossZ - rotationZ * quaternionVectorCrossY;
    const secondCrossY = rotationZ * quaternionVectorCrossX - rotationX * quaternionVectorCrossZ;
    const secondCrossZ = rotationX * quaternionVectorCrossY - rotationY * quaternionVectorCrossX;
    return [x + 2 * (rotationW * quaternionVectorCrossX + secondCrossX), y + 2 * (rotationW * quaternionVectorCrossY + secondCrossY), z + 2 * (rotationW * quaternionVectorCrossZ + secondCrossZ)];
  };
  const rotatedAxisOfAxis = { x: rotateVector([1, 0, 0]), y: rotateVector([0, 1, 0]), z: rotateVector([0, 0, 1]) };
  const composeProjectionAxis = (projectionAxis) => ({
    x: projectionAxis.x * rotatedAxisOfAxis.x[0] + projectionAxis.y * rotatedAxisOfAxis.x[1] + projectionAxis.z * rotatedAxisOfAxis.x[2],
    y: projectionAxis.x * rotatedAxisOfAxis.y[0] + projectionAxis.y * rotatedAxisOfAxis.y[1] + projectionAxis.z * rotatedAxisOfAxis.y[2],
    z: projectionAxis.x * rotatedAxisOfAxis.z[0] + projectionAxis.y * rotatedAxisOfAxis.z[1] + projectionAxis.z * rotatedAxisOfAxis.z[2],
  });
  return getCubeGeometryFromProjectionAxes(composeProjectionAxis(projectionAxisOfName.screenX), composeProjectionAxis(projectionAxisOfName.screenY), composeProjectionAxis(projectionAxisOfName.depth));
};

// The body-diagonal effect rotates the object basis around normalized (1,1,1), then uses the same camera projection and depth math as every other view.
// The other cards use the same math with the other three normalized body diagonals.
const getCubeGeometryWithAxisRotation = (topAngleDegrees, horizontalAngleDegrees, rotationDegrees, rotationAxis) => {
  const rotationAxisLength = Math.hypot(...rotationAxis);
  const halfRotationRadians = rotationDegrees * Math.PI / 360;
  const rotationScale = Math.sin(halfRotationRadians) / rotationAxisLength;
  const rotationQuaternion = [rotationAxis[0] * rotationScale, rotationAxis[1] * rotationScale, rotationAxis[2] * rotationScale, Math.cos(halfRotationRadians)];
  return getCubeGeometryWithQuaternionRotation(topAngleDegrees, horizontalAngleDegrees, rotationQuaternion);
};

const formatPoint = ([x, y]) => `${roundCoordinate(x)} ${roundCoordinate(y)}`;

const getCubePathData = (cubeGeometry) => {
  const cubeEdges = [
    { vertices: ["000", "100"], faces: [["y", 0], ["z", 0]] },
    { vertices: ["010", "110"], faces: [["y", 1], ["z", 0]] },
    { vertices: ["001", "101"], faces: [["y", 0], ["z", 1]] },
    { vertices: ["011", "111"], faces: [["y", 1], ["z", 1]] },
    { vertices: ["000", "010"], faces: [["x", 0], ["z", 0]] },
    { vertices: ["100", "110"], faces: [["x", 1], ["z", 0]] },
    { vertices: ["001", "011"], faces: [["x", 0], ["z", 1]] },
    { vertices: ["101", "111"], faces: [["x", 1], ["z", 1]] },
    { vertices: ["000", "001"], faces: [["x", 0], ["y", 0]] },
    { vertices: ["100", "101"], faces: [["x", 1], ["y", 0]] },
    { vertices: ["010", "011"], faces: [["x", 0], ["y", 1]] },
    { vertices: ["110", "111"], faces: [["x", 1], ["y", 1]] },
  ];
  let cubePathData = "";

  for (const { vertices, faces } of cubeEdges) {
    const isEdgeVisible = faces.some(([axis, side]) => side === 1 ? cubeGeometry.depthOfAxis[axis] > 0 : cubeGeometry.depthOfAxis[axis] < 0);
    if (isEdgeVisible) {
      cubePathData += `M${formatPoint(cubeGeometry.vertexOfKey[vertices[0]])}L${formatPoint(cubeGeometry.vertexOfKey[vertices[1]])}`;
    }
  }

  return cubePathData;
};

// -------------------------- K cuts and extrusion --------------------------

const getKPolygonPoints = (parameterValues, inletPointCenterProgress) => {
  const { a0, a1, b0, c0, c1, d0, e0, e1 } = parameterValues;
  const topInletPointX = a0 + (a1 - a0) * inletPointCenterProgress / 2;
  const bottomInletPointX = c0 + (c1 - c0) * inletPointCenterProgress / 2;
  return [[0, 0], [a0, 0], [topInletPointX, b0], [a1, 0], [1, 0], [1 - e0, e1], [1, 1], [c1, 1], [bottomInletPointX, 1 - d0], [c0, 1], [0, 1]];
};

const projectCubePoint = ([x, y, z], cubeGeometry) => {
  const rawX = x * cubeGeometry.screenXOfAxis.x + y * cubeGeometry.screenXOfAxis.y + z * cubeGeometry.screenXOfAxis.z;
  const rawY = x * cubeGeometry.screenYOfAxis.x + y * cubeGeometry.screenYOfAxis.y + z * cubeGeometry.screenYOfAxis.z;
  const depth = x * cubeGeometry.depthOfAxis.x + y * cubeGeometry.depthOfAxis.y + z * cubeGeometry.depthOfAxis.z;
  return {
    depth,
    screen: [CUBE_CENTER + (rawX - cubeGeometry.rawCenterX) * cubeGeometry.cubeScale, CUBE_CENTER + (rawY - cubeGeometry.rawCenterY) * cubeGeometry.cubeScale],
  };
};

const getTriangleIndices = (polygonPoints) => {
  const remainingIndices = polygonPoints.map((_, index) => index);
  const triangleIndices = [];
  const polygonArea = polygonPoints.reduce((area, [x, y], index) => {
    const [nextX, nextY] = polygonPoints[(index + 1) % polygonPoints.length];
    return area + x * nextY - nextX * y;
  }, 0);
  const orientation = Math.sign(polygonArea);

  while (remainingIndices.length > 3) {
    let didFindEar = false;

    for (let remainingIndex = 0; remainingIndex < remainingIndices.length; remainingIndex += 1) {
      const previousIndex = remainingIndices[(remainingIndex - 1 + remainingIndices.length) % remainingIndices.length];
      const currentIndex = remainingIndices[remainingIndex];
      const nextIndex = remainingIndices[(remainingIndex + 1) % remainingIndices.length];
      const previousPoint = polygonPoints[previousIndex];
      const currentPoint = polygonPoints[currentIndex];
      const nextPoint = polygonPoints[nextIndex];
      const cornerCross = (currentPoint[0] - previousPoint[0]) * (nextPoint[1] - currentPoint[1]) - (currentPoint[1] - previousPoint[1]) * (nextPoint[0] - currentPoint[0]);

      if (cornerCross * orientation <= 0.000000001) {
        continue;
      }

      const doesEarContainPoint = remainingIndices.some((pointIndex) => {
        if (pointIndex === previousIndex || pointIndex === currentIndex || pointIndex === nextIndex) {
          return false;
        }
        const point = polygonPoints[pointIndex];
        const crossA = (currentPoint[0] - previousPoint[0]) * (point[1] - previousPoint[1]) - (currentPoint[1] - previousPoint[1]) * (point[0] - previousPoint[0]);
        const crossB = (nextPoint[0] - currentPoint[0]) * (point[1] - currentPoint[1]) - (nextPoint[1] - currentPoint[1]) * (point[0] - currentPoint[0]);
        const crossC = (previousPoint[0] - nextPoint[0]) * (point[1] - nextPoint[1]) - (previousPoint[1] - nextPoint[1]) * (point[0] - nextPoint[0]);
        return crossA * orientation >= -0.000000001 && crossB * orientation >= -0.000000001 && crossC * orientation >= -0.000000001;
      });

      if (!doesEarContainPoint) {
        triangleIndices.push([previousIndex, currentIndex, nextIndex]);
        remainingIndices.splice(remainingIndex, 1);
        didFindEar = true;
        break;
      }
    }

    if (!didFindEar) {
      break;
    }
  }

  if (remainingIndices.length === 3) {
    triangleIndices.push([...remainingIndices]);
  }

  return triangleIndices;
};

// Extrusion alignment places the requested depth against the front, around the center, or against the back of the cube.
const getExtrusionOffsets = (extrusionDepth, extrusionAlignment) => {
  if (extrusionAlignment === "front") {
    return [0, extrusionDepth];
  }
  if (extrusionAlignment === "center") {
    return [(1 - extrusionDepth) / 2, (1 + extrusionDepth) / 2];
  }
  if (extrusionAlignment === "back") {
    return [1 - extrusionDepth, 1];
  }
  throw new Error("The extrusion alignment is invalid.");
};

// Each mesh edge is split at projected triangle boundaries, then tested against surface depth one segment at a time.
const getKPrismSvgSource = (kPolygonPoints, faceName, extrusionDepth, extrusionAlignment, cubeGeometry, shouldShowHiddenEdges, hiddenEdgeOpacity, hiddenEdgeStrokeScale, creaseTransition, strokeWidth, strokeColor, faceColor, edgeEffects) => {
  const { renderId, shouldUseTransparentFaces, shouldUseDepthShading, depthShadingAmount, faceDepthGradientAmount, shouldUseNeonGlow, glowRadius, glowIntensity, glowColor, shouldUseStrokeGradient, gradientStartColor, gradientEndColor, gradientAngle } = edgeEffects;
  const { movingStrokeScale, movingOpacity, stationaryStrokeScale, stationaryOpacity } = creaseTransition;
  const surfaceKPolygonPointEntries = kPolygonPoints.map((point, kPolygonPointIndex) => ({ point, kPolygonPointIndex })).filter(({ point: [x, y] }, pointIndex) => {
    const [previousX, previousY] = kPolygonPoints[(pointIndex - 1 + kPolygonPoints.length) % kPolygonPoints.length];
    return Math.abs(x - previousX) > K_POLYGON_SIMPLIFY_EPSILON || Math.abs(y - previousY) > K_POLYGON_SIMPLIFY_EPSILON;
  });
  const surfaceKPolygonPoints = surfaceKPolygonPointEntries.map(({ point }) => point);
  let simplifiedKPolygonPointIndices = surfaceKPolygonPoints.map((_, pointIndex) => pointIndex);
  let didRemoveStraightPoint = true;

  // Collapsed K cuts become an ordinary rectangle, so zero-area faces never reach triangulation or occlusion testing.
  while (didRemoveStraightPoint && simplifiedKPolygonPointIndices.length > 3) {
    const pointsBeforeSimplifying = simplifiedKPolygonPointIndices.length;
    simplifiedKPolygonPointIndices = simplifiedKPolygonPointIndices.filter((surfacePointIndex, pointIndex) => {
      const [x, y] = surfaceKPolygonPoints[surfacePointIndex];
      const [previousX, previousY] = surfaceKPolygonPoints[simplifiedKPolygonPointIndices[(pointIndex - 1 + simplifiedKPolygonPointIndices.length) % simplifiedKPolygonPointIndices.length]];
      const [nextX, nextY] = surfaceKPolygonPoints[simplifiedKPolygonPointIndices[(pointIndex + 1) % simplifiedKPolygonPointIndices.length]];
      const cross = (x - previousX) * (nextY - y) - (y - previousY) * (nextX - x);
      const dot = (x - previousX) * (x - nextX) + (y - previousY) * (y - nextY);
      return Math.abs(cross) > K_POLYGON_SIMPLIFY_EPSILON || dot > K_POLYGON_SIMPLIFY_EPSILON;
    });
    didRemoveStraightPoint = simplifiedKPolygonPointIndices.length < pointsBeforeSimplifying;
  }

  const simplifiedKPolygonPoints = simplifiedKPolygonPointIndices.map((pointIndex) => surfaceKPolygonPoints[pointIndex]);
  const minimumSimplifiedX = Math.min(...simplifiedKPolygonPoints.map(([x]) => x));
  const maximumSimplifiedX = Math.max(...simplifiedKPolygonPoints.map(([x]) => x));
  const minimumSimplifiedY = Math.min(...simplifiedKPolygonPoints.map(([, y]) => y));
  const maximumSimplifiedY = Math.max(...simplifiedKPolygonPoints.map(([, y]) => y));
  const shouldRenderSimplifiedCube = simplifiedKPolygonPoints.length === 4 && simplifiedKPolygonPoints.every(([x, y]) => (Math.abs(x - minimumSimplifiedX) < K_POLYGON_SIMPLIFY_EPSILON || Math.abs(x - maximumSimplifiedX) < K_POLYGON_SIMPLIFY_EPSILON) && (Math.abs(y - minimumSimplifiedY) < K_POLYGON_SIMPLIFY_EPSILON || Math.abs(y - maximumSimplifiedY) < K_POLYGON_SIMPLIFY_EPSILON));

  // A fully collapsed K uses the four rectangle corners for the mesh so its walls become four complete cube faces instead of many shaded strips.
  const faceBoundaryPointIndices = shouldRenderSimplifiedCube ? simplifiedKPolygonPointIndices : surfaceKPolygonPoints.map((_, pointIndex) => pointIndex);
  const [extrusionFrontOffset, extrusionBackOffset] = getExtrusionOffsets(extrusionDepth, extrusionAlignment);
  const frontPoints3D = surfaceKPolygonPoints.map(([x, y]) => faceName === "left" ? [x, y, 1 - extrusionFrontOffset] : [1 - extrusionFrontOffset, y, 1 - x]);
  const backPoints3D = surfaceKPolygonPoints.map(([x, y]) => faceName === "left" ? [x, y, 1 - extrusionBackOffset] : [1 - extrusionBackOffset, y, 1 - x]);
  const vertices = [...frontPoints3D, ...backPoints3D].map((point3D) => ({ point3D, ...projectCubePoint(point3D, cubeGeometry) }));
  const pointCount = surfaceKPolygonPoints.length;
  const frontTriangleIndices = getTriangleIndices(simplifiedKPolygonPoints).map((triangle) => triangle.map((pointIndex) => simplifiedKPolygonPointIndices[pointIndex]));
  const triangles = [];
  const edges = [];
  // Only the four outside cube corners remain structural when the K cuts collapse, so every other front-to-back edge stays fade-controlled throughout the morph.
  const cubeCornerKPolygonPointIndices = new Set([0, 4, 6, 10]);
  // The inlet tips and center joint move during the morph, while the remaining K crease points stay in place.
  const movingKPolygonPointIndices = new Set([2, 5, 8]);
  const faceVertexIndices = [faceBoundaryPointIndices, faceBoundaryPointIndices.map((pointIndex) => pointCount + pointIndex)];
  const faceTriangleIndices = [frontTriangleIndices, frontTriangleIndices.map(([indexA, indexB, indexC]) => [pointCount + indexA, pointCount + indexC, pointCount + indexB])];

  for (const [indexA, indexB, indexC] of frontTriangleIndices) {
    triangles.push([indexA, indexB, indexC]);
    triangles.push([pointCount + indexA, pointCount + indexC, pointCount + indexB]);
  }

  for (let pointIndex = 0; pointIndex < pointCount; pointIndex += 1) {
    const nextPointIndex = (pointIndex + 1) % pointCount;
    const sideTriangleIndices = [[pointIndex, pointCount + pointIndex, pointCount + nextPointIndex], [pointIndex, pointCount + nextPointIndex, nextPointIndex]];
    triangles.push(...sideTriangleIndices);
    edges.push({ indices: [pointIndex, nextPointIndex], isSurfaceDetail: false, shouldShrinkSurfaceDetail: false }, { indices: [pointCount + pointIndex, pointCount + nextPointIndex], isSurfaceDetail: false, shouldShrinkSurfaceDetail: false }, { indices: [pointIndex, pointCount + pointIndex], isSurfaceDetail: !cubeCornerKPolygonPointIndices.has(surfaceKPolygonPointEntries[pointIndex].kPolygonPointIndex), shouldShrinkSurfaceDetail: movingKPolygonPointIndices.has(surfaceKPolygonPointEntries[pointIndex].kPolygonPointIndex) });
    if (!shouldRenderSimplifiedCube) {
      faceVertexIndices.push([pointIndex, pointCount + pointIndex, pointCount + nextPointIndex, nextPointIndex]);
      faceTriangleIndices.push(sideTriangleIndices);
    }
  }

  if (shouldRenderSimplifiedCube) {
    for (let faceBoundaryIndex = 0; faceBoundaryIndex < faceBoundaryPointIndices.length; faceBoundaryIndex += 1) {
      const pointIndex = faceBoundaryPointIndices[faceBoundaryIndex];
      const nextPointIndex = faceBoundaryPointIndices[(faceBoundaryIndex + 1) % faceBoundaryPointIndices.length];
      faceVertexIndices.push([pointIndex, pointCount + pointIndex, pointCount + nextPointIndex, nextPointIndex]);
      faceTriangleIndices.push([[pointIndex, pointCount + pointIndex, pointCount + nextPointIndex], [pointIndex, pointCount + nextPointIndex, nextPointIndex]]);
    }
  }

  let visiblePathData = "";
  let visibleSurfaceDetailPathData = "";
  let visibleFadingSurfaceDetailPathData = "";
  let hiddenPathData = "";
  let hiddenSurfaceDetailPathData = "";
  let hiddenFadingSurfaceDetailPathData = "";
  const visibleVertexIndices = new Set();
  const hiddenVertexIndices = new Set();
  const hiddenSurfaceDetailVertexIndices = new Set();
  const hiddenFadingSurfaceDetailVertexIndices = new Set();
  const cross2D = ([xA, yA], [xB, yB]) => xA * yB - yA * xB;

  for (const { indices: [edgeStartIndex, edgeEndIndex], isSurfaceDetail, shouldShrinkSurfaceDetail } of edges) {
    const edgeStart = vertices[edgeStartIndex];
    const edgeEnd = vertices[edgeEndIndex];
    const edgeVector = [edgeEnd.screen[0] - edgeStart.screen[0], edgeEnd.screen[1] - edgeStart.screen[1]];
    const splitParameters = [0, 1];

    for (const triangle of triangles) {
      for (let triangleEdgeIndex = 0; triangleEdgeIndex < 3; triangleEdgeIndex += 1) {
        const triangleEdgeStart = vertices[triangle[triangleEdgeIndex]].screen;
        const triangleEdgeEnd = vertices[triangle[(triangleEdgeIndex + 1) % 3]].screen;
        const triangleEdgeVector = [triangleEdgeEnd[0] - triangleEdgeStart[0], triangleEdgeEnd[1] - triangleEdgeStart[1]];
        const startDifference = [triangleEdgeStart[0] - edgeStart.screen[0], triangleEdgeStart[1] - edgeStart.screen[1]];
        const denominator = cross2D(edgeVector, triangleEdgeVector);

        if (Math.abs(denominator) < 0.0000001) {
          continue;
        }

        const edgeParameter = cross2D(startDifference, triangleEdgeVector) / denominator;
        const triangleEdgeParameter = cross2D(startDifference, edgeVector) / denominator;
        if (edgeParameter > 0.000001 && edgeParameter < 0.999999 && triangleEdgeParameter >= -0.000001 && triangleEdgeParameter <= 1.000001) {
          splitParameters.push(edgeParameter);
        }
      }
    }

    splitParameters.sort((parameterA, parameterB) => parameterA - parameterB);
    const uniqueSplitParameters = splitParameters.filter((parameter, index) => index === 0 || Math.abs(parameter - splitParameters[index - 1]) > 0.000001);
    const visibilityRuns = [];
    let runStartParameter = uniqueSplitParameters[0];
    let isRunHidden;

    for (let splitIndex = 0; splitIndex < uniqueSplitParameters.length - 1; splitIndex += 1) {
      const parameterStart = uniqueSplitParameters[splitIndex];
      const parameterEnd = uniqueSplitParameters[splitIndex + 1];
      const parameterMiddle = (parameterStart + parameterEnd) / 2;
      const middleScreen = [edgeStart.screen[0] + edgeVector[0] * parameterMiddle, edgeStart.screen[1] + edgeVector[1] * parameterMiddle];
      const middleDepth = edgeStart.depth + (edgeEnd.depth - edgeStart.depth) * parameterMiddle;
      let isHidden = false;

      for (const triangle of triangles) {
        const triangleA = vertices[triangle[0]];
        const triangleB = vertices[triangle[1]];
        const triangleC = vertices[triangle[2]];
        const denominator = (triangleB.screen[1] - triangleC.screen[1]) * (triangleA.screen[0] - triangleC.screen[0]) + (triangleC.screen[0] - triangleB.screen[0]) * (triangleA.screen[1] - triangleC.screen[1]);
        if (Math.abs(denominator) < 0.0000001) {
          continue;
        }
        const weightA = ((triangleB.screen[1] - triangleC.screen[1]) * (middleScreen[0] - triangleC.screen[0]) + (triangleC.screen[0] - triangleB.screen[0]) * (middleScreen[1] - triangleC.screen[1])) / denominator;
        const weightB = ((triangleC.screen[1] - triangleA.screen[1]) * (middleScreen[0] - triangleC.screen[0]) + (triangleA.screen[0] - triangleC.screen[0]) * (middleScreen[1] - triangleC.screen[1])) / denominator;
        const weightC = 1 - weightA - weightB;

        if (weightA < -0.000001 || weightB < -0.000001 || weightC < -0.000001) {
          continue;
        }

        const triangleDepth = triangleA.depth * weightA + triangleB.depth * weightB + triangleC.depth * weightC;
        if (triangleDepth > middleDepth + 0.000001) {
          isHidden = true;
          break;
        }
      }

      if (isRunHidden === undefined) {
        isRunHidden = isHidden;
      } else if (isHidden !== isRunHidden) {
        visibilityRuns.push({ parameterStart: runStartParameter, parameterEnd: parameterStart, isHidden: isRunHidden });
        runStartParameter = parameterStart;
        isRunHidden = isHidden;
      }
    }

    visibilityRuns.push({ parameterStart: runStartParameter, parameterEnd: uniqueSplitParameters[uniqueSplitParameters.length - 1], isHidden: isRunHidden });
    const hiddenDetailVertexIndices = shouldShrinkSurfaceDetail ? hiddenSurfaceDetailVertexIndices : hiddenFadingSurfaceDetailVertexIndices;

    // Adjacent fragments with the same visibility share one SVG segment so antialiasing cannot leave seams between them.
    for (const { parameterStart, parameterEnd, isHidden } of visibilityRuns) {
      const runStart = [edgeStart.screen[0] + edgeVector[0] * parameterStart, edgeStart.screen[1] + edgeVector[1] * parameterStart];
      const runEnd = [edgeStart.screen[0] + edgeVector[0] * parameterEnd, edgeStart.screen[1] + edgeVector[1] * parameterEnd];
      const runPathData = `M${formatPoint(runStart)}L${formatPoint(runEnd)}`;
      if (isHidden) {
        if (isSurfaceDetail) {
          if (shouldShrinkSurfaceDetail) {
            hiddenSurfaceDetailPathData += runPathData;
          } else {
            hiddenFadingSurfaceDetailPathData += runPathData;
          }
        } else {
          hiddenPathData += runPathData;
        }
        if (parameterStart < 0.000001) {
          (isSurfaceDetail ? hiddenDetailVertexIndices : hiddenVertexIndices).add(edgeStartIndex);
        }
        if (parameterEnd > 0.999999) {
          (isSurfaceDetail ? hiddenDetailVertexIndices : hiddenVertexIndices).add(edgeEndIndex);
        }
      } else {
        if (isSurfaceDetail) {
          if (shouldShrinkSurfaceDetail) {
            visibleSurfaceDetailPathData += runPathData;
          } else {
            visibleFadingSurfaceDetailPathData += runPathData;
          }
        } else {
          visiblePathData += runPathData;
        }
        if (parameterStart < 0.000001) {
          visibleVertexIndices.add(edgeStartIndex);
        }
        if (parameterEnd > 0.999999) {
          visibleVertexIndices.add(edgeEndIndex);
        }
      }
    }
  }

  const gradientAngleRadians = gradientAngle * Math.PI / 180;
  const gradientDirectionX = Math.cos(gradientAngleRadians) * VIEW_BOX_SIZE / 2;
  const gradientDirectionY = Math.sin(gradientAngleRadians) * VIEW_BOX_SIZE / 2;
  const gradientId = `${renderId}-stroke-gradient`;
  const glowId = `${renderId}-neon-glow`;
  const gradientSource = shouldUseStrokeGradient ? `<linearGradient id="${gradientId}" gradientUnits="userSpaceOnUse" x1="${roundCoordinate(CUBE_CENTER - gradientDirectionX)}" y1="${roundCoordinate(CUBE_CENTER - gradientDirectionY)}" x2="${roundCoordinate(CUBE_CENTER + gradientDirectionX)}" y2="${roundCoordinate(CUBE_CENTER + gradientDirectionY)}"><stop offset="0" stop-color="${gradientStartColor}" /><stop offset="1" stop-color="${gradientEndColor}" /></linearGradient>` : "";
  // TODO: AI-PICKED-VALUE: One-third of the selected radius makes a strong inner halo while the full radius keeps the soft outer glow.
  const glowInnerRadius = roundCoordinate(glowRadius / 3);
  const glowDefinitionSource = shouldUseNeonGlow ? `<filter id="${glowId}" filterUnits="userSpaceOnUse" x="0" y="0" width="${VIEW_BOX_SIZE}" height="${VIEW_BOX_SIZE}" color-interpolation-filters="sRGB"><feGaussianBlur in="SourceAlpha" stdDeviation="${glowRadius}" result="outer-blur" /><feGaussianBlur in="SourceAlpha" stdDeviation="${glowInnerRadius}" result="inner-blur" /><feFlood flood-color="${glowColor}" flood-opacity="${glowIntensity}" result="glow-color" /><feComposite in="glow-color" in2="outer-blur" operator="in" result="outer-glow" /><feComposite in="glow-color" in2="inner-blur" operator="in" result="inner-glow" /><feMerge><feMergeNode in="outer-glow" /><feMergeNode in="inner-glow" /><feMergeNode in="SourceGraphic" /></feMerge></filter>` : "";
  const strokePaint = shouldUseStrokeGradient ? `url(#${gradientId})` : strokeColor;
  const faces = faceVertexIndices.map((vertexIndices, faceIndex) => ({ faceIndex, vertexIndices, triangleIndices: faceTriangleIndices[faceIndex], depth: vertexIndices.reduce((sum, vertexIndex) => sum + vertices[vertexIndex].depth, 0) / vertexIndices.length })).sort((faceA, faceB) => faceA.depth - faceB.depth);
  const minimumFaceDepth = faces[0].depth;
  const maximumFaceDepth = faces[faces.length - 1].depth;
  const faceDepthRange = maximumFaceDepth - minimumFaceDepth;
  const minimumVertexDepth = Math.min(...vertices.map(({ depth }) => depth));
  const maximumVertexDepth = Math.max(...vertices.map(({ depth }) => depth));
  const vertexDepthRange = maximumVertexDepth - minimumVertexDepth;
  const faceColorChannels = [1, 3, 5].map((channelStart) => Number.parseInt(faceColor.slice(channelStart, channelStart + 2), 16));
  const getShadedFaceColor = (shadeAmount) => `#${faceColorChannels.map((channel) => Math.round(channel * (1 - shadeAmount)).toString(16).padStart(2, "0")).join("")}`;
  const facePaintOfFaceIndex = {};
  let faceGradientSource = "";

  // Each face receives one paint treatment before its triangles are clipped against every nearer surface.
  for (const { faceIndex, vertexIndices, depth } of faces) {
    const depthProgress = faceDepthRange < K_POLYGON_SIMPLIFY_EPSILON ? 1 : (depth - minimumFaceDepth) / faceDepthRange;
    const shadeAmount = shouldUseDepthShading ? depthShadingAmount * (1 - depthProgress) : 0;
    let facePaint = getShadedFaceColor(shadeAmount);

    if (shouldUseDepthShading && faceDepthGradientAmount > 0 && vertexDepthRange > K_POLYGON_SIMPLIFY_EPSILON) {
      const faceDepths = vertexIndices.map((vertexIndex) => vertices[vertexIndex].depth);
      const minimumDepth = Math.min(...faceDepths);
      const maximumDepth = Math.max(...faceDepths);
      const currentFaceDepthRange = maximumDepth - minimumDepth;
      const vertexA = vertices[vertexIndices[0]];
      let depthGradientX;
      let depthGradientY;

      for (let vertexBIndex = 1; vertexBIndex < vertexIndices.length - 1; vertexBIndex += 1) {
        const vertexB = vertices[vertexIndices[vertexBIndex]];
        for (let vertexCIndex = vertexBIndex + 1; vertexCIndex < vertexIndices.length; vertexCIndex += 1) {
          const vertexC = vertices[vertexIndices[vertexCIndex]];
          const screenBX = vertexB.screen[0] - vertexA.screen[0];
          const screenBY = vertexB.screen[1] - vertexA.screen[1];
          const screenCX = vertexC.screen[0] - vertexA.screen[0];
          const screenCY = vertexC.screen[1] - vertexA.screen[1];
          const determinant = screenBX * screenCY - screenCX * screenBY;
          if (Math.abs(determinant) > K_POLYGON_SIMPLIFY_EPSILON) {
            const depthB = vertexB.depth - vertexA.depth;
            const depthC = vertexC.depth - vertexA.depth;
            depthGradientX = (depthB * screenCY - depthC * screenBY) / determinant;
            depthGradientY = (screenBX * depthC - screenCX * depthB) / determinant;
            break;
          }
        }
        if (depthGradientX !== undefined) {
          break;
        }
      }

      const depthGradientLengthSquared = depthGradientX === undefined ? 0 : depthGradientX * depthGradientX + depthGradientY * depthGradientY;
      if (currentFaceDepthRange > K_POLYGON_SIMPLIFY_EPSILON && depthGradientLengthSquared > K_POLYGON_SIMPLIFY_EPSILON) {
        const minimumDepthVertex = vertices[vertexIndices[faceDepths.indexOf(minimumDepth)]];
        const gradientEndX = minimumDepthVertex.screen[0] + depthGradientX * currentFaceDepthRange / depthGradientLengthSquared;
        const gradientEndY = minimumDepthVertex.screen[1] + depthGradientY * currentFaceDepthRange / depthGradientLengthSquared;
        const gradientShadeRange = faceDepthGradientAmount * currentFaceDepthRange / vertexDepthRange;
        const farFaceColor = getShadedFaceColor(Math.min(1, shadeAmount + gradientShadeRange / 2));
        const nearFaceColor = getShadedFaceColor(Math.max(0, shadeAmount - gradientShadeRange / 2));
        const faceGradientId = `${renderId}-face-depth-gradient-${faceIndex}`;
        faceGradientSource += `<linearGradient id="${faceGradientId}" gradientUnits="userSpaceOnUse" x1="${roundCoordinate(minimumDepthVertex.screen[0])}" y1="${roundCoordinate(minimumDepthVertex.screen[1])}" x2="${roundCoordinate(gradientEndX)}" y2="${roundCoordinate(gradientEndY)}"><stop offset="0" stop-color="${farFaceColor}" /><stop offset="1" stop-color="${nearFaceColor}" /></linearGradient>`;
        facePaint = `url(#${faceGradientId})`;
      }
    }

    facePaintOfFaceIndex[faceIndex] = facePaint;
  }

  const clipPolygonToDepthSide = (polygonPoints, getValue, minimumValue) => {
    if (polygonPoints.length === 0) {
      return [];
    }

    const clippedPoints = [];
    let previousPoint = polygonPoints[polygonPoints.length - 1];
    let previousValue = getValue(previousPoint);
    let wasPreviousInside = previousValue >= minimumValue;

    for (const currentPoint of polygonPoints) {
      const currentValue = getValue(currentPoint);
      const isCurrentInside = currentValue >= minimumValue;
      if (isCurrentInside !== wasPreviousInside) {
        const intersectionProgress = (minimumValue - previousValue) / (currentValue - previousValue);
        clippedPoints.push([previousPoint[0] + (currentPoint[0] - previousPoint[0]) * intersectionProgress, previousPoint[1] + (currentPoint[1] - previousPoint[1]) * intersectionProgress]);
      }
      if (isCurrentInside) {
        clippedPoints.push(currentPoint);
      }
      previousPoint = currentPoint;
      previousValue = currentValue;
      wasPreviousInside = isCurrentInside;
    }

    return clippedPoints;
  };

  const visibleFaceTriangles = [];
  for (const { faceIndex, triangleIndices } of faces) {
    for (const triangleIndicesForFace of triangleIndices) {
      const triangleVertices = triangleIndicesForFace.map((vertexIndex) => vertices[vertexIndex]);
      const [vertexA, vertexB, vertexC] = triangleVertices;
      const screenBX = vertexB.screen[0] - vertexA.screen[0];
      const screenBY = vertexB.screen[1] - vertexA.screen[1];
      const screenCX = vertexC.screen[0] - vertexA.screen[0];
      const screenCY = vertexC.screen[1] - vertexA.screen[1];
      const screenArea = screenBX * screenCY - screenCX * screenBY;
      if (screenArea <= K_POLYGON_SIMPLIFY_EPSILON) {
        continue;
      }
      const depthB = vertexB.depth - vertexA.depth;
      const depthC = vertexC.depth - vertexA.depth;
      const screenPoints = triangleVertices.map(({ screen }) => screen);
      visibleFaceTriangles.push({
        faceIndex,
        screenPoints,
        vertexA,
        depthGradientX: (depthB * screenCY - depthC * screenBY) / screenArea,
        depthGradientY: (screenBX * depthC - screenCX * depthB) / screenArea,
        minimumDepth: Math.min(...triangleVertices.map(({ depth }) => depth)),
        maximumDepth: Math.max(...triangleVertices.map(({ depth }) => depth)),
        minimumScreenX: Math.min(...screenPoints.map(([screenX]) => screenX)),
        maximumScreenX: Math.max(...screenPoints.map(([screenX]) => screenX)),
        minimumScreenY: Math.min(...screenPoints.map(([, screenY]) => screenY)),
        maximumScreenY: Math.max(...screenPoints.map(([, screenY]) => screenY)),
      });
    }
  }

  let faceOcclusionMaskSource = "";
  const faceSource = shouldUseTransparentFaces ? "" : visibleFaceTriangles.map((faceTriangle, faceTriangleIndex) => {
    const facePathData = `${faceTriangle.screenPoints.map((screenPoint, index) => `${index === 0 ? "M" : "L"}${formatPoint(screenPoint)}`).join("")}Z`;
    const getFaceTriangleDepth = ([screenX, screenY]) => faceTriangle.vertexA.depth + faceTriangle.depthGradientX * (screenX - faceTriangle.vertexA.screen[0]) + faceTriangle.depthGradientY * (screenY - faceTriangle.vertexA.screen[1]);
    let occlusionPathSource = "";

    for (const occludingTriangle of visibleFaceTriangles) {
      if (occludingTriangle.faceIndex === faceTriangle.faceIndex || occludingTriangle.maximumDepth <= faceTriangle.minimumDepth + K_POLYGON_SIMPLIFY_EPSILON || occludingTriangle.maximumScreenX < faceTriangle.minimumScreenX || occludingTriangle.minimumScreenX > faceTriangle.maximumScreenX || occludingTriangle.maximumScreenY < faceTriangle.minimumScreenY || occludingTriangle.minimumScreenY > faceTriangle.maximumScreenY) {
        continue;
      }

      let overlapPolygon = faceTriangle.screenPoints;
      for (let clipEdgeIndex = 0; clipEdgeIndex < 3 && overlapPolygon.length > 0; clipEdgeIndex += 1) {
        const clipStart = occludingTriangle.screenPoints[clipEdgeIndex];
        const clipEnd = occludingTriangle.screenPoints[(clipEdgeIndex + 1) % 3];
        overlapPolygon = clipPolygonToDepthSide(overlapPolygon, ([screenX, screenY]) => (clipEnd[0] - clipStart[0]) * (screenY - clipStart[1]) - (clipEnd[1] - clipStart[1]) * (screenX - clipStart[0]), -K_POLYGON_SIMPLIFY_EPSILON);
      }

      if (overlapPolygon.length < 3) {
        continue;
      }

      const getOccludingTriangleDepth = ([screenX, screenY]) => occludingTriangle.vertexA.depth + occludingTriangle.depthGradientX * (screenX - occludingTriangle.vertexA.screen[0]) + occludingTriangle.depthGradientY * (screenY - occludingTriangle.vertexA.screen[1]);
      overlapPolygon = clipPolygonToDepthSide(overlapPolygon, (screenPoint) => getOccludingTriangleDepth(screenPoint) - getFaceTriangleDepth(screenPoint), K_POLYGON_SIMPLIFY_EPSILON);
      const overlapArea = overlapPolygon.reduce((area, [screenX, screenY], pointIndex) => {
        const [nextScreenX, nextScreenY] = overlapPolygon[(pointIndex + 1) % overlapPolygon.length];
        return area + screenX * nextScreenY - nextScreenX * screenY;
      }, 0);
      if (Math.abs(overlapArea) > K_POLYGON_SIMPLIFY_EPSILON) {
        occlusionPathSource += `<path d="${overlapPolygon.map((screenPoint, index) => `${index === 0 ? "M" : "L"}${formatPoint(screenPoint)}`).join("")}Z" fill="black" stroke="black" stroke-width="${FACE_SEAM_STROKE_WIDTH}" stroke-linejoin="round" />`;
      }
    }

    let maskAttribute = "";
    if (occlusionPathSource !== "") {
      const faceMaskId = `${renderId}-face-occlusion-${faceTriangleIndex}`;
      faceOcclusionMaskSource += `<mask id="${faceMaskId}" maskUnits="userSpaceOnUse" x="0" y="0" width="${VIEW_BOX_SIZE}" height="${VIEW_BOX_SIZE}"><path d="${facePathData}" fill="white" stroke="white" stroke-width="${FACE_SEAM_STROKE_WIDTH}" stroke-linejoin="round" />${occlusionPathSource}</mask>`;
      maskAttribute = ` mask="url(#${faceMaskId})"`;
    }

    const facePaint = facePaintOfFaceIndex[faceTriangle.faceIndex];
    return `<path d="${facePathData}" fill="${facePaint}" stroke="${facePaint}" stroke-width="${FACE_SEAM_STROKE_WIDTH}" stroke-linejoin="round"${maskAttribute} />`;
  }).join("");
  const definitionContent = gradientSource + glowDefinitionSource + faceGradientSource + faceOcclusionMaskSource;
  const definitionsSource = definitionContent === "" ? "" : `<defs>${definitionContent}</defs>`;
  const movingSurfaceDetailStrokeWidth = roundCoordinate(strokeWidth * movingStrokeScale);
  const stationarySurfaceDetailStrokeWidth = roundCoordinate(strokeWidth * stationaryStrokeScale);
  const hiddenStrokeWidth = roundCoordinate(strokeWidth * hiddenEdgeStrokeScale);
  const hiddenMovingSurfaceDetailStrokeWidth = roundCoordinate(movingSurfaceDetailStrokeWidth * hiddenEdgeStrokeScale);
  const hiddenStationarySurfaceDetailStrokeWidth = roundCoordinate(stationarySurfaceDetailStrokeWidth * hiddenEdgeStrokeScale);
  const hiddenSource = `<path d="${hiddenPathData}" fill="none" stroke="${strokePaint}" stroke-width="${hiddenStrokeWidth}" stroke-linecap="butt" stroke-linejoin="round" />`;
  const hiddenSurfaceDetailSource = `<path d="${hiddenSurfaceDetailPathData}" fill="none" stroke="${strokePaint}" stroke-width="${hiddenMovingSurfaceDetailStrokeWidth}" stroke-linecap="butt" stroke-linejoin="round" opacity="${movingOpacity}" />`;
  const hiddenFadingSurfaceDetailSource = `<path d="${hiddenFadingSurfaceDetailPathData}" fill="none" stroke="${strokePaint}" stroke-width="${hiddenStationarySurfaceDetailStrokeWidth}" stroke-linecap="butt" stroke-linejoin="round" opacity="${stationaryOpacity}" />`;
  const hiddenVertexSource = [...hiddenVertexIndices].map((vertexIndex) => `<circle cx="${roundCoordinate(vertices[vertexIndex].screen[0])}" cy="${roundCoordinate(vertices[vertexIndex].screen[1])}" r="${hiddenStrokeWidth / 2}" fill="${strokePaint}" />`).join("");
  const hiddenSurfaceDetailVertexSource = [...hiddenSurfaceDetailVertexIndices].filter((vertexIndex) => !hiddenVertexIndices.has(vertexIndex)).map((vertexIndex) => `<circle cx="${roundCoordinate(vertices[vertexIndex].screen[0])}" cy="${roundCoordinate(vertices[vertexIndex].screen[1])}" r="${hiddenMovingSurfaceDetailStrokeWidth / 2}" fill="${strokePaint}" opacity="${movingOpacity}" />`).join("");
  const hiddenFadingSurfaceDetailVertexSource = [...hiddenFadingSurfaceDetailVertexIndices].filter((vertexIndex) => !hiddenVertexIndices.has(vertexIndex)).map((vertexIndex) => `<circle cx="${roundCoordinate(vertices[vertexIndex].screen[0])}" cy="${roundCoordinate(vertices[vertexIndex].screen[1])}" r="${hiddenStationarySurfaceDetailStrokeWidth / 2}" fill="${strokePaint}" opacity="${stationaryOpacity}" />`).join("");
  const hiddenGroupSource = shouldShowHiddenEdges ? `<g opacity="${hiddenEdgeOpacity}">${hiddenSource}${hiddenSurfaceDetailSource}${hiddenFadingSurfaceDetailSource}${hiddenVertexSource}${hiddenSurfaceDetailVertexSource}${hiddenFadingSurfaceDetailVertexSource}</g>` : "";
  const visibleEdgeSource = `<path d="${visiblePathData}" fill="none" stroke="${strokePaint}" stroke-width="${strokeWidth}" stroke-linecap="butt" stroke-linejoin="round" /><path d="${visibleSurfaceDetailPathData}" fill="none" stroke="${strokePaint}" stroke-width="${movingSurfaceDetailStrokeWidth}" stroke-linecap="butt" stroke-linejoin="round" opacity="${movingOpacity}" /><path d="${visibleFadingSurfaceDetailPathData}" fill="none" stroke="${strokePaint}" stroke-width="${stationarySurfaceDetailStrokeWidth}" stroke-linecap="butt" stroke-linejoin="round" opacity="${stationaryOpacity}" />`;
  const visibleVertexSource = [...visibleVertexIndices].map((vertexIndex) => `<circle cx="${roundCoordinate(vertices[vertexIndex].screen[0])}" cy="${roundCoordinate(vertices[vertexIndex].screen[1])}" r="${strokeWidth / 2}" fill="${strokePaint}" />`).join("");
  const visibleSource = shouldUseNeonGlow ? `<g filter="url(#${glowId})">${visibleEdgeSource}${visibleVertexSource}</g>` : `${visibleEdgeSource}${visibleVertexSource}`;
  return `${definitionsSource}${faceSource}${hiddenGroupSource}${visibleSource}`;
};

const EFFECT_MORPH_GEOMETRY_END = 0.7;
const getCubeMorphState = (boxProgress) => ({
  kGeometryScale: 1 - Math.min(1, boxProgress / EFFECT_MORPH_GEOMETRY_END),
  surfaceDetailStrokeScale: 1 - Math.min(1, boxProgress / EFFECT_MORPH_GEOMETRY_END),
  twoStepSurfaceDetailScale: 1 - Math.max(0, (boxProgress - EFFECT_MORPH_GEOMETRY_END) / (1 - EFFECT_MORPH_GEOMETRY_END)),
});

// Every morph uses one crease transition so changing the control affects hover and click animations in both directions.
const getCreaseTransition = (shouldAnimate, transitionMode, cubeMorphState) => {
  if (!shouldAnimate) {
    const endpointVisibility = cubeMorphState.kGeometryScale === 0 ? 0 : 1;
    return { movingStrokeScale: endpointVisibility, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: endpointVisibility };
  }
  if (transitionMode === "fade") {
    return { movingStrokeScale: 1, movingOpacity: cubeMorphState.surfaceDetailStrokeScale, stationaryStrokeScale: 1, stationaryOpacity: cubeMorphState.surfaceDetailStrokeScale };
  }
  if (transitionMode === "shrink") {
    return { movingStrokeScale: cubeMorphState.surfaceDetailStrokeScale, movingOpacity: 1, stationaryStrokeScale: cubeMorphState.surfaceDetailStrokeScale, stationaryOpacity: 1 };
  }
  if (transitionMode === "two-step") {
    return { movingStrokeScale: cubeMorphState.twoStepSurfaceDetailScale, movingOpacity: cubeMorphState.twoStepSurfaceDetailScale, stationaryStrokeScale: cubeMorphState.twoStepSurfaceDetailScale, stationaryOpacity: cubeMorphState.twoStepSurfaceDetailScale };
  }
  if (transitionMode === "mixed") {
    return { movingStrokeScale: cubeMorphState.surfaceDetailStrokeScale, movingOpacity: 1, stationaryStrokeScale: 1, stationaryOpacity: cubeMorphState.surfaceDetailStrokeScale };
  }
  throw new Error("The crease transition mode is invalid.");
};

// Positive Y is measured around the camera's up axis, and its minimum size prevents the other components from looking like a reversal.
// The free copies pass negative one as the minimum component, which expands the same sampling across the full rotation sphere.
const getRandomAngularVelocity = (minimumSpeed, maximumSpeed, projectionAxisOfName, minimumCameraUpComponent) => {
  const directionScreenY = minimumCameraUpComponent + Math.random() * (1 - minimumCameraUpComponent);
  const directionLongitude = Math.random() * Math.PI * 2;
  const directionHorizontalLength = Math.sqrt(1 - directionScreenY * directionScreenY);
  const directionScreenX = Math.cos(directionLongitude) * directionHorizontalLength;
  const directionDepth = Math.sin(directionLongitude) * directionHorizontalLength;
  const speed = minimumSpeed + Math.random() * (maximumSpeed - minimumSpeed);
  return [
    (projectionAxisOfName.screenX.x * directionScreenX + projectionAxisOfName.screenY.x * directionScreenY + projectionAxisOfName.depth.x * directionDepth) * speed,
    (projectionAxisOfName.screenX.y * directionScreenX + projectionAxisOfName.screenY.y * directionScreenY + projectionAxisOfName.depth.y * directionDepth) * speed,
    (projectionAxisOfName.screenX.z * directionScreenX + projectionAxisOfName.screenY.z * directionScreenY + projectionAxisOfName.depth.z * directionDepth) * speed,
  ];
};

// Each frame adds a small world-space rotation to the stored orientation and normalizes it to prevent long-running drift.
const getUpdatedRotationQuaternion = (rotationQuaternion, angularVelocity, elapsedSeconds) => {
  const angularSpeed = Math.hypot(...angularVelocity);
  if (angularSpeed < Number.EPSILON) {
    return rotationQuaternion;
  }
  const halfRotationRadians = angularSpeed * elapsedSeconds * Math.PI / 360;
  const rotationScale = Math.sin(halfRotationRadians) / angularSpeed;
  const [incrementX, incrementY, incrementZ, incrementW] = [angularVelocity[0] * rotationScale, angularVelocity[1] * rotationScale, angularVelocity[2] * rotationScale, Math.cos(halfRotationRadians)];
  const [rotationX, rotationY, rotationZ, rotationW] = rotationQuaternion;
  const updatedRotationQuaternion = [
    incrementW * rotationX + incrementX * rotationW + incrementY * rotationZ - incrementZ * rotationY,
    incrementW * rotationY - incrementX * rotationZ + incrementY * rotationW + incrementZ * rotationX,
    incrementW * rotationZ + incrementX * rotationY - incrementY * rotationX + incrementZ * rotationW,
    incrementW * rotationW - incrementX * rotationX - incrementY * rotationY - incrementZ * rotationZ,
  ];
  const quaternionLength = Math.hypot(...updatedRotationQuaternion);
  return updatedRotationQuaternion.map((component) => component / quaternionLength);
};

export {
  VIEW_BOX_SIZE,
  HIDDEN_EDGE_OPACITY,
  getCubeGeometry,
  getCubeGeometryWithQuaternionRotation,
  getCubeGeometryWithAxisRotation,
  getCubeProjectionAxes,
  getKPolygonPoints,
  getKPrismSvgSource,
  getCubeMorphState,
  getCreaseTransition,
  getRandomAngularVelocity,
  getUpdatedRotationQuaternion,
};
