// The Kube.new logo mark: a K carved out of a cube. This file turns the design decisions
// (camera angles, colours, the hover animations) into SVG by calling the ported generator in
// cubeGeometry.js. Components only need renderKubeLogoSvg and headerHoverCycle from here.

import {
    HIDDEN_EDGE_OPACITY,
    VIEW_BOX_SIZE,
    getCreaseTransition,
    getCubeGeometry,
    getCubeGeometryWithAxisRotation,
    getCubeGeometryWithQuaternionRotation,
    getCubeMorphState,
    getCubeProjectionAxes,
    getKPolygonPoints,
    getKPrismSvgSource,
    getRandomAngularVelocity,
    getUpdatedRotationQuaternion,
    type CubeVector,
    type KShapeParameters,
    type RotationQuaternion,
} from './cubeGeometry.js'

// -------------------------- Design constants --------------------------

/** Where the camera sits: how much of the top face shows, and how far around the cube we are. */
export type LogoView = { topAngle: number, horizontalAngle: number }

/** Every mark rests here, almost face-on, which is where the K reads most clearly. */
export const restingKView: LogoView = { topAngle: 5, horizontalAngle: 80 }

/** Both marks land the closed cube here. */
export const cubeView: LogoView = { topAngle: 22, horizontalAngle: 45 }

/** The wide-stem K with the deeper top inlet, matching the editor's landed shape. */
const kShape: KShapeParameters = { a0: 0.33, a1: 0.6, b0: 0.34, c0: 0.33, c1: 0.5, d0: 0.22, e0: 0.39, e1: 0.5 }

/** One hover takes this long to play, and the same again to return. */
export const logoMorphMilliseconds = 650

/** Colour and weight for one look. `strokeWidth` is in viewBox units, as the editor writes it. */
export type LogoPreset = {
    strokeWidth: number
    strokeColor: string
    faceColor: string
    depthShadingAmount: number
    glowIntensity: number
    glowRadius: number
    glowColor: string
    gradientStartColor: string
    gradientEndColor: string
    gradientAngle: number
}

/** The editor presets the animations move between, copied across unchanged. */
export const logoPresetOfName: { [presetName: string]: LogoPreset } = {
    lavenderCrisp: {
        strokeWidth: 8, strokeColor: '#4c00ff', faceColor: '#f4f0ff', depthShadingAmount: 0,
        glowIntensity: 0.25, glowRadius: 1.5, glowColor: '#336dff',
        gradientStartColor: '#4c00ff', gradientEndColor: '#33a7ff', gradientAngle: 42,
    },
    monoOutlineThick: {
        strokeWidth: 6, strokeColor: '#000000', faceColor: '#ffffff', depthShadingAmount: 0,
        glowIntensity: 0, glowRadius: 6, glowColor: '#2f6bff',
        gradientStartColor: '#000000', gradientEndColor: '#000000', gradientAngle: 45,
    },
    monoHairline: {
        strokeWidth: 2.5, strokeColor: '#000000', faceColor: '#ffffff', depthShadingAmount: 0,
        glowIntensity: 0, glowRadius: 6, glowColor: '#2f6bff',
        gradientStartColor: '#000000', gradientEndColor: '#000000', gradientAngle: 45,
    },
    softGrayDepth: {
        strokeWidth: 2, strokeColor: '#000000', faceColor: '#f2f2f2', depthShadingAmount: 0.35,
        glowIntensity: 0, glowRadius: 6, glowColor: '#2f6bff',
        gradientStartColor: '#000000', gradientEndColor: '#000000', gradientAngle: 45,
    },
}

/** The look the mark rests in, everywhere it appears. */
export const restingLogoPreset = logoPresetOfName.lavenderCrisp

// -------------------------- Blending --------------------------

/** Smoothstep, the same easing the editor's hover animations use. */
export const getEasedProgress = (progress: number) => progress * progress * (3 - 2 * progress)

const mix = (from: number, to: number, amount: number) => from + (to - from) * amount

/** Blends two hex colours channel by channel, so a preset change reads as a colour move. */
const mixColor = (fromHex: string, toHex: string, amount: number) => {
    const getChannels = (hex: string) => [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16))
    const toChannels = getChannels(toHex)

    return '#' + getChannels(fromHex)
        .map((channel, index) => Math.round(mix(channel, toChannels[index], amount)).toString(16).padStart(2, '0'))
        .join('')
}

const mixPreset = (fromPreset: LogoPreset, toPreset: LogoPreset, amount: number): LogoPreset => ({
    strokeWidth: mix(fromPreset.strokeWidth, toPreset.strokeWidth, amount),
    strokeColor: mixColor(fromPreset.strokeColor, toPreset.strokeColor, amount),
    faceColor: mixColor(fromPreset.faceColor, toPreset.faceColor, amount),
    depthShadingAmount: mix(fromPreset.depthShadingAmount, toPreset.depthShadingAmount, amount),
    glowIntensity: mix(fromPreset.glowIntensity, toPreset.glowIntensity, amount),
    glowRadius: mix(fromPreset.glowRadius, toPreset.glowRadius, amount),
    glowColor: mixColor(fromPreset.glowColor, toPreset.glowColor, amount),
    gradientStartColor: mixColor(fromPreset.gradientStartColor, toPreset.gradientStartColor, amount),
    gradientEndColor: mixColor(fromPreset.gradientEndColor, toPreset.gradientEndColor, amount),
    gradientAngle: mix(fromPreset.gradientAngle, toPreset.gradientAngle, amount),
})

const mixView = (fromView: LogoView, toView: LogoView, amount: number): LogoView => ({
    topAngle: mix(fromView.topAngle, toView.topAngle, amount),
    horizontalAngle: mix(fromView.horizontalAngle, toView.horizontalAngle, amount),
})

/** Square on to the camera: the orientation every mark rests in. */
const UNROTATED: RotationQuaternion = [0, 0, 0, 1]

/**
 * Blends one orientation into another.
 *
 * Every orientation can be written two ways, as a quaternion or as that same quaternion with
 * every component negated, and only one of the two is on the short way round from where we are.
 * Once the mark has tumbled past half a turn it is holding the far one, and blending straight to
 * it sends the mark all the way around the houses to reach an angle it is practically already at.
 * Flipping to the near one first is what keeps the way there the shorter of the two.
 */
const mixQuaternions = (fromQuaternion: RotationQuaternion, toQuaternion: RotationQuaternion, amount: number): RotationQuaternion => {
    const isFarWayRound = fromQuaternion.reduce((dot, component, index) => dot + component * toQuaternion[index], 0) < 0
    const nearToQuaternion = isFarWayRound ? toQuaternion.map((component) => -component) : toQuaternion
    const blended = fromQuaternion.map((component, index) => component + (nearToQuaternion[index] - component) * amount)
    const blendedLength = Math.hypot(...blended)

    return [blended[0] / blendedLength, blended[1] / blendedLength, blended[2] / blendedLength, blended[3] / blendedLength]
}

// -------------------------- Rendering one frame --------------------------

/** Everything that can differ between two moments of an animation. */
export type LogoFrame = {
    view: LogoView
    /** 0 is the open K, 1 is the closed cube. */
    boxProgress: number
    /** 1 runs the prism through the whole cube; lower makes a shallower slab. */
    extrusionDepth: number
    /** Where a shallower slab sits: hard against the front face, or halfway into the cube. */
    extrusionAlignment: 'front' | 'center'
    preset: LogoPreset
    /** Spin around one of the cube's own diagonals. */
    axisRotation: { degrees: number, axis: CubeVector } | null
    /** Free orientation, used by the tumbling marks. */
    rotationQuaternion: RotationQuaternion | null
    /** How strongly the edges hidden behind the shape show through. 0 leaves them out. */
    hiddenEdgeOpacity: number
}

/** The frame a mark sits in when nothing is happening to it. */
export const getRestingFrame = (view: LogoView): LogoFrame => ({
    view,
    boxProgress: 0,
    extrusionDepth: 1,
    extrusionAlignment: 'front',
    preset: restingLogoPreset,
    axisRotation: null,
    rotationQuaternion: null,
    hiddenEdgeOpacity: 0,
})

/**
 * Draws one frame as a complete SVG string.
 *
 * `renderId` must be unique per mark on the page, because the generator uses it to name the
 * SVG filter and mask ids. `strokeScale` compensates the preset's stroke for small marks: the
 * editor authors strokes against a 256 viewBox, so a 34px header mark needs them scaled up.
 */
export const renderKubeLogoSvg = ({ frame, renderId, sidePx, strokeScale }: {
    frame: LogoFrame
    renderId: string
    sidePx: number
    strokeScale: number
}) => {
    const { view, boxProgress, extrusionDepth, extrusionAlignment, preset, axisRotation, rotationQuaternion, hiddenEdgeOpacity } = frame

    // Closing the cuts shrinks the three inlet depths and slides their tips to the midpoints.
    const cubeMorphState = getCubeMorphState(boxProgress)
    const kPolygonPoints = getKPolygonPoints({
        ...kShape,
        b0: kShape.b0 * cubeMorphState.kGeometryScale,
        d0: kShape.d0 * cubeMorphState.kGeometryScale,
        e0: kShape.e0 * cubeMorphState.kGeometryScale,
    }, 1 - cubeMorphState.kGeometryScale)

    const cubeGeometry = rotationQuaternion !== null
        ? getCubeGeometryWithQuaternionRotation(view.topAngle, view.horizontalAngle, rotationQuaternion)
        : axisRotation !== null
            ? getCubeGeometryWithAxisRotation(view.topAngle, view.horizontalAngle, axisRotation.degrees, axisRotation.axis)
            : getCubeGeometry(view.topAngle, view.horizontalAngle)

    const prismSource = getKPrismSvgSource(
        kPolygonPoints, 'left', extrusionDepth, extrusionAlignment, cubeGeometry,
        hiddenEdgeOpacity > 0, hiddenEdgeOpacity, 1,
        getCreaseTransition(true, 'two-step', cubeMorphState),
        preset.strokeWidth * strokeScale, preset.strokeColor, preset.faceColor,
        {
            renderId,
            shouldUseTransparentFaces: false,
            shouldUseDepthShading: preset.depthShadingAmount > 0.001,
            depthShadingAmount: preset.depthShadingAmount,
            faceDepthGradientAmount: 0.15,
            shouldUseNeonGlow: preset.glowIntensity > 0.001,
            glowRadius: preset.glowRadius,
            glowIntensity: preset.glowIntensity,
            glowColor: preset.glowColor,
            shouldUseStrokeGradient: true,
            gradientStartColor: preset.gradientStartColor,
            gradientEndColor: preset.gradientEndColor,
            gradientAngle: preset.gradientAngle,
        },
    )

    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VIEW_BOX_SIZE} ${VIEW_BOX_SIZE}"`
        + ` width="${sidePx}" height="${sidePx}" fill="none" aria-hidden="true">${prismSource}</svg>`
}

// -------------------------- Levelling the K --------------------------

/** Builds the orientation that turns `degrees` around `axis`. */
const getRotationQuaternionAroundAxis = (axis: CubeVector, degrees: number): RotationQuaternion => {
    const halfRadians = degrees * Math.PI / 360
    const axisScale = Math.sin(halfRadians) / Math.hypot(...axis)

    return [axis[0] * axisScale, axis[1] * axisScale, axis[2] * axisScale, Math.cos(halfRadians)]
}

/** Turns by `second` and then by `first`, in the same order the generator's own drift does. */
const multiplyQuaternions = (first: RotationQuaternion, second: RotationQuaternion): RotationQuaternion => {
    const [firstX, firstY, firstZ, firstW] = first
    const [secondX, secondY, secondZ, secondW] = second

    return [
        firstW * secondX + firstX * secondW + firstY * secondZ - firstZ * secondY,
        firstW * secondY - firstX * secondZ + firstY * secondW + firstZ * secondX,
        firstW * secondZ + firstX * secondY - firstY * secondX + firstZ * secondW,
        firstW * secondW - firstX * secondX - firstY * secondY - firstZ * secondZ,
    ]
}

/**
 * How far the mark has to turn for the bottom of the K's legs to run flat across the page. The
 * legs end along the cube's own x axis, so their slope is however far that axis lands from
 * horizontal once the camera has projected it.
 */
export const getLevelLegsRollDegrees = (view: LogoView) => {
    const projectionAxisOfName = getCubeProjectionAxes(view.topAngle, view.horizontalAngle)

    return -Math.atan2(projectionAxisOfName.screenY.x, projectionAxisOfName.screenX.x) * 180 / Math.PI
}

/**
 * Levels the K by turning the whole shape around the line the camera looks down, which leaves
 * the drawing rolled in place while the glow and the stroke gradient stay fixed to the page. A
 * plain rotation of the finished drawing would take those with it.
 *
 * `levelAmount` runs from 1 at rest to 0 once a hover has fully played. That is what keeps the
 * levelling out of the movement itself: the mark sits level, unwinds as it sets off, and the
 * cube or the tumble it arrives at is exactly the one it would have reached anyway.
 */
export const getLevelLegsFrame = (frame: LogoFrame, levelAmount: number): LogoFrame => {
    const { depth } = getCubeProjectionAxes(frame.view.topAngle, frame.view.horizontalAngle)
    const rollQuaternion = getRotationQuaternionAroundAxis(
        [depth.x, depth.y, depth.z],
        getLevelLegsRollDegrees(frame.view) * levelAmount,
    )

    // Whichever way the frame was already turned, the roll goes on top of it as one orientation,
    // because the generator can only be handed a single one.
    const turnedQuaternion = frame.rotationQuaternion !== null
        ? frame.rotationQuaternion
        : frame.axisRotation !== null
            ? getRotationQuaternionAroundAxis(frame.axisRotation.axis, frame.axisRotation.degrees)
            : null

    return {
        ...frame,
        axisRotation: null,
        rotationQuaternion: turnedQuaternion === null
            ? rollQuaternion
            : multiplyQuaternions(rollQuaternion, turnedQuaternion),
    }
}

// -------------------------- The drifting cube --------------------------

/** What the drifting cube remembers between frames. */
export type WanderState = {
    /** Where the mark is facing right now. This is what gets drawn, tumbling or going home. */
    rotationQuaternion: RotationQuaternion
    angularVelocity: CubeVector
    targetAngularVelocity: CubeVector
    targetChangeTimeMs: number
    /** Whether the pointer has gone and the mark is on its way back to square. */
    isGoingHome: boolean
    /** Where it was facing when the pointer left, and how far back towards square it has come. */
    returnFromQuaternion: RotationQuaternion
    returnProgress: number
}

export const createWanderState = (): WanderState => ({
    rotationQuaternion: UNROTATED,
    angularVelocity: [0, 0, 0],
    targetAngularVelocity: [0, 0, 0],
    targetChangeTimeMs: 0,
    isGoingHome: false,
    returnFromQuaternion: UNROTATED,
    returnProgress: 1,
})

/** True once the mark has finished going home, so a caller knows it can stop drawing frames. */
export const getIsWanderSettled = (wanderState: WanderState) => wanderState.returnProgress === 1

/** How fast a tumble runs, in degrees per second. Both ranges come from the editor. */
export type WanderSpeedRange = { minimumSpeed: number, maximumSpeed: number }

export const steadyWanderSpeed: WanderSpeedRange = { minimumSpeed: 18, maximumSpeed: 55 }
export const fastWanderSpeed: WanderSpeedRange = { minimumSpeed: 40, maximumSpeed: 95 }

// TODO: AI-PICKED-VALUE: These match the editor's inertial wander so the drift feels the same here.
const wanderResponseSeconds = 0.65
const wanderTargetMinimumMs = 2000
const wanderTargetMaximumMs = 10000

/**
 * Moves a tumbling mark on by one frame. It picks a fresh random spin direction every two to
 * ten seconds and eases towards it, which reads as the shape turning over in the hand. Passing
 * -1 as the lowest allowed up-component is what lets it tumble in every direction rather than
 * always drifting rightwards, which is the editor's "free" variant.
 *
 * With `isGoingHome` it turns back to square instead, over one morph, from wherever it had got
 * to when the pointer left. Coming back to it half way home simply carries on tumbling from the
 * angle it is at: nothing is retraced, because where it is going home *from* is remembered on the
 * way out rather than worked out from how far through the hover it is. Deriving it from the hover
 * is what used to make flicking on and off swing the mark about — every trip back out started
 * from the same place however far round it had already come.
 */
export const advanceWanderState = ({ wanderState, currentTimeMs, elapsedSeconds, speedRange, isGoingHome }: {
    wanderState: WanderState
    currentTimeMs: number
    elapsedSeconds: number
    speedRange: WanderSpeedRange
    isGoingHome: boolean
}) => {
    if (isGoingHome) {
        // Only the frame the pointer actually left on decides where home is being set off from.
        // Asking whether it had finished instead would start it over the moment it arrived, and
        // it would never be able to report itself done.
        if (!wanderState.isGoingHome) {
            wanderState.isGoingHome = true
            wanderState.returnFromQuaternion = wanderState.rotationQuaternion
            wanderState.returnProgress = 0
        }
        wanderState.returnProgress = Math.min(1, wanderState.returnProgress + elapsedSeconds * 1000 / logoMorphMilliseconds)
        wanderState.rotationQuaternion =
            mixQuaternions(wanderState.returnFromQuaternion, UNROTATED, getEasedProgress(wanderState.returnProgress))
        return
    }

    // Whatever it is facing now is where it tumbles on from, so a hover picked back up part way
    // home neither snaps nor has to catch up. It does pick a fresh direction though, rather than
    // resuming the one it happened to be heading in when the pointer left, so a second look at
    // the mark is a new tumble rather than the last one continued.
    if (wanderState.isGoingHome) {
        wanderState.targetChangeTimeMs = 0
    }
    wanderState.isGoingHome = false
    wanderState.returnProgress = 1

    if (currentTimeMs >= wanderState.targetChangeTimeMs) {
        const projectionAxisOfName = getCubeProjectionAxes(cubeView.topAngle, cubeView.horizontalAngle)
        wanderState.targetAngularVelocity =
            getRandomAngularVelocity(speedRange.minimumSpeed, speedRange.maximumSpeed, projectionAxisOfName, -1)
        wanderState.targetChangeTimeMs = currentTimeMs + wanderTargetMinimumMs
            + Math.random() * (wanderTargetMaximumMs - wanderTargetMinimumMs)
    }

    const velocityBlend = 1 - Math.exp(-elapsedSeconds / wanderResponseSeconds)
    for (let axisIndex = 0; axisIndex < 3; axisIndex += 1) {
        wanderState.angularVelocity[axisIndex] +=
            (wanderState.targetAngularVelocity[axisIndex] - wanderState.angularVelocity[axisIndex]) * velocityBlend
    }
    wanderState.rotationQuaternion =
        getUpdatedRotationQuaternion(wanderState.rotationQuaternion, wanderState.angularVelocity, elapsedSeconds)
}

// -------------------------- The hover animations --------------------------

/**
 * One thing the mark can do when hovered. `frameAt` maps eased progress, where 0 is resting
 * and 1 is fully played, to the frame to draw. An effect that keeps moving for as long as the
 * pointer stays on it gives a `wanderSpeedRange`; the rest leave it null and simply play out.
 */
export type LogoHoverEffect = {
    label: string
    wanderSpeedRange: WanderSpeedRange | null
    frameAt: (progress: number, context: { baseView: LogoView, wanderState: WanderState }) => LogoFrame
}

/**
 * The header mark steps through these in order, one per hover, so the same gesture never plays
 * the same thing twice running. Every angle is relative to the mark's own resting view, which
 * lets the header and the fold sit their K differently while sharing these definitions.
 */
export const headerHoverCycle: LogoHoverEffect[] = [
    {
        label: 'Cube',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(mixView(baseView, cubeView, progress)),
            boxProgress: progress,
        }),
    },
    {
        label: 'Shallow extrusion',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(baseView),
            extrusionDepth: mix(1, 0.3, progress),
        }),
    },
    {
        label: 'Horizontal spin',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => getRestingFrame({
            topAngle: baseView.topAngle,
            horizontalAngle: baseView.horizontalAngle + progress * 360,
        }),
    },
    {
        label: 'Vertical spin',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => getRestingFrame({
            topAngle: baseView.topAngle + progress * 360,
            horizontalAngle: baseView.horizontalAngle,
        }),
    },
    {
        label: 'Body-diagonal spin',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(baseView),
            axisRotation: { degrees: progress * 360, axis: [1, 1, -1] },
        }),
    },
    {
        label: 'Black and white hairline',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(baseView),
            preset: mixPreset(restingLogoPreset, logoPresetOfName.monoHairline, progress),
        }),
    },
    {
        label: 'Mono outline turning away',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(mixView(baseView, { topAngle: 20, horizontalAngle: 110 }, progress)),
            preset: mixPreset(restingLogoPreset, logoPresetOfName.monoOutlineThick, progress),
        }),
    },
    {
        label: 'Mono outline turning towards',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(mixView(baseView, { topAngle: 20, horizontalAngle: 50 }, progress)),
            preset: mixPreset(restingLogoPreset, logoPresetOfName.monoOutlineThick, progress),
        }),
    },
    {
        label: 'Drifting gray cube',
        wanderSpeedRange: steadyWanderSpeed,
        frameAt: (progress, { baseView, wanderState }) => ({
            ...getRestingFrame(mixView(baseView, cubeView, progress)),
            boxProgress: progress,
            rotationQuaternion: wanderState.rotationQuaternion,
            preset: mixPreset(restingLogoPreset, logoPresetOfName.softGrayDepth, progress),
            hiddenEdgeOpacity: HIDDEN_EDGE_OPACITY * progress,
        }),
    },
]

/**
 * The K keeps its own colours and stays a K, but its hidden back edges fade up and it tumbles
 * freely for as long as the pointer rests on it. Because the K is an extruded shape rather than
 * a plain cube, showing the hidden edges makes the whole prism readable as it turns.
 *
 * The turn is whatever the drift has got to, taken as it is. Going back to square is the drift's
 * own business rather than something blended in by how far through the hover we are.
 */
export const tumblingKHoverEffect: LogoHoverEffect = {
    label: 'Tumbling K with hidden edges',
    wanderSpeedRange: fastWanderSpeed,
    frameAt: (progress, { baseView, wanderState }) => ({
        ...getRestingFrame(baseView),
        rotationQuaternion: wanderState.rotationQuaternion,
        hiddenEdgeOpacity: HIDDEN_EDGE_OPACITY * progress,
    }),
}

/** What the header runs today. `headerHoverCycle` above is kept so it can be swapped back in. */
export const headerTumbleCycle: LogoHoverEffect[] = [tumblingKHoverEffect]

/** The fold plays only the fold-to-cube move, and clicking it opens the K again. */
export const foldHoverCycle: LogoHoverEffect[] = [headerHoverCycle[0]]

/**
 * The fold run the other way: at rest this is the closed cube, and hovering opens the K out of it.
 * Give the mark an introMilliseconds as well, because a mark that is never hovered is drawn from
 * its resting view, which is the K - the intro plays this backwards once on load and leaves it
 * sitting as the cube.
 */
export const unfoldHoverCycle: LogoHoverEffect[] = [
    {
        label: 'Unfold to K',
        wanderSpeedRange: null,
        frameAt: (progress, { baseView }) => ({
            ...getRestingFrame(mixView(cubeView, baseView, progress)),
            boxProgress: 1 - progress,
        }),
    },
]

/** The resting mark, for anywhere that wants it without any hover behaviour. */
export const renderRestingKubeLogoSvg = ({ view, renderId, sidePx, strokeScale, isKLevel, preset }: {
    view: LogoView
    renderId: string
    sidePx: number
    strokeScale: number
    isKLevel: boolean
    preset: LogoPreset
}) => {
    const restingFrame = { ...getRestingFrame(view), preset }

    return renderKubeLogoSvg({
        frame: isKLevel ? getLevelLegsFrame(restingFrame, 1) : restingFrame,
        renderId,
        sidePx,
        strokeScale,
    })
}

/**
 * How much of its square the resting mark actually draws in, as fractions of that square.
 *
 * The generator fits a whole cube to the square, and a K standing on one face of that cube only
 * fills the middle of it — roughly three fifths across and half down. Anything that should sit
 * tight against the mark rather than against the empty square around it, which means both the
 * region that answers to the pointer and the link the mark sits in, has to measure the drawing
 * instead of taking the square at its word.
 *
 * Measured from the drawing itself so it keeps up with the shape and the camera angle, rather
 * than being a pair of numbers somebody read off once.
 */
export const getRestingMarkBounds = ({ view, strokeScale, isKLevel, preset }: {
    view: LogoView
    strokeScale: number
    isKLevel: boolean
    preset: LogoPreset
}) => {
    // Only the path data is worth measuring: the vertex dots sit on path ends, and everything
    // else in the drawing is a filter or a gradient rather than a mark on the page.
    const svgSource = renderRestingKubeLogoSvg({ view, renderId: 'mark-bounds', sidePx: VIEW_BOX_SIZE, strokeScale, isKLevel, preset })
    const drawnCoordinates = [...svgSource.matchAll(/ d="([^"]*)"/g)]
        .flatMap((pathMatch) => pathMatch[1].match(/-?[\d.]+/g) ?? [])
        .map(Number)
    const drawnXs = drawnCoordinates.filter((_, coordinateIndex) => coordinateIndex % 2 === 0)
    const drawnYs = drawnCoordinates.filter((_, coordinateIndex) => coordinateIndex % 2 === 1)

    // The lines are drawn centred on those coordinates, so half a stroke hangs off every edge.
    const strokeOverhang = preset.strokeWidth * strokeScale / 2
    const left = Math.min(...drawnXs) - strokeOverhang
    const top = Math.min(...drawnYs) - strokeOverhang
    const right = Math.max(...drawnXs) + strokeOverhang
    const bottom = Math.max(...drawnYs) + strokeOverhang

    return {
        left: left / VIEW_BOX_SIZE,
        top: top / VIEW_BOX_SIZE,
        width: (right - left) / VIEW_BOX_SIZE,
        height: (bottom - top) / VIEW_BOX_SIZE,
    }
}
