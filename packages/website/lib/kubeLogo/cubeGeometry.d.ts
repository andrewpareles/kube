// Types for the ported generator in cubeGeometry.js. Only the parts the website calls are
// described here; everything else in that file stays private to it.

/** A point projected onto the 256x256 viewBox. */
export type ScreenPoint = [number, number]

/** A direction in the cube's own space, before projection. */
export type CubeVector = [number, number, number]

/** Orientation as [x, y, z, w]. */
export type RotationQuaternion = [number, number, number, number]

/** The three cube axes after projection, each carrying its screen offset and its depth. */
export type CubeProjectionAxes = {
    screenX: { x: number, y: number, z: number }
    screenY: { x: number, y: number, z: number }
    depth: { x: number, y: number, z: number }
}

/**
 * A projected cube. It carries more than this — the silhouette corners, the projection axes and
 * the scale it was fitted at — but the website only ever passes it straight back to the
 * generator, so only the corner lookup is described here.
 */
export type CubeGeometry = {
    /** The eight projected corners, keyed by their "000".."111" corner code. */
    vertexOfKey: { [cornerKey: string]: ScreenPoint }
}

/** The eight numbers that shape the K, each running 0 to 1. */
export type KShapeParameters = {
    a0: number, a1: number, b0: number, c0: number
    c1: number, d0: number, e0: number, e1: number
}

/** How far through the K-to-cube fold the shape is. */
export type CubeMorphState = {
    kGeometryScale: number
    surfaceDetailStrokeScale: number
    twoStepSurfaceDetailScale: number
}

/** How the fold edges shrink and fade while the shape moves. */
export type CreaseTransition = {
    movingStrokeScale: number
    movingOpacity: number
    stationaryStrokeScale: number
    stationaryOpacity: number
}

/** Colour and glow settings for one render. `renderId` keeps SVG filter ids unique per mark. */
export type EdgeEffects = {
    renderId: string
    shouldUseTransparentFaces: boolean
    shouldUseDepthShading: boolean
    depthShadingAmount: number
    faceDepthGradientAmount: number
    shouldUseNeonGlow: boolean
    glowRadius: number
    glowIntensity: number
    glowColor: string
    shouldUseStrokeGradient: boolean
    gradientStartColor: string
    gradientEndColor: string
    gradientAngle: number
}

export const VIEW_BOX_SIZE: number
export const HIDDEN_EDGE_OPACITY: number

export const getCubeGeometry: (topAngleDegrees: number, horizontalAngleDegrees: number) => CubeGeometry

export const getCubeGeometryWithQuaternionRotation: (
    topAngleDegrees: number,
    horizontalAngleDegrees: number,
    rotationQuaternion: RotationQuaternion,
) => CubeGeometry

export const getCubeGeometryWithAxisRotation: (
    topAngleDegrees: number,
    horizontalAngleDegrees: number,
    rotationDegrees: number,
    rotationAxis: CubeVector,
) => CubeGeometry

export const getCubeProjectionAxes: (topAngleDegrees: number, horizontalAngleDegrees: number) => CubeProjectionAxes

export const getKPolygonPoints: (
    parameterValues: KShapeParameters,
    inletPointCenterProgress: number,
) => ScreenPoint[]

export const getKPrismSvgSource: (
    kPolygonPoints: ScreenPoint[],
    faceName: string,
    extrusionDepth: number,
    extrusionAlignment: string,
    cubeGeometry: CubeGeometry,
    shouldShowHiddenEdges: boolean,
    hiddenEdgeOpacity: number,
    hiddenEdgeStrokeScale: number,
    creaseTransition: CreaseTransition,
    strokeWidth: number,
    strokeColor: string,
    faceColor: string,
    edgeEffects: EdgeEffects,
) => string

export const getCubeMorphState: (boxProgress: number) => CubeMorphState

export const getCreaseTransition: (
    shouldAnimate: boolean,
    transitionMode: string,
    cubeMorphState: CubeMorphState,
) => CreaseTransition

export const getRandomAngularVelocity: (
    minimumSpeed: number,
    maximumSpeed: number,
    projectionAxisOfName: CubeProjectionAxes,
    minimumCameraUpComponent: number,
) => CubeVector

export const getUpdatedRotationQuaternion: (
    rotationQuaternion: RotationQuaternion,
    angularVelocity: CubeVector,
    elapsedSeconds: number,
) => RotationQuaternion
