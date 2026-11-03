// This file should pretty much mirror the rust file with the same name, except for places I indicated.

import {
    Color3,
    DataId,
    Location3D,
    ObjectId,
    Orientation3D,
    PhysicsInfo3D,
    PhysicsToggles3D,
    UserId,
    Vec3,
    VisualSettings,
} from "./shared"

export type ObjectLocation = "Server" | "Client"

/** 
 * If paused: animationTimeSecs is the duration into the animation to show.
 * 
 * If playing: localStartMs is the timestamp at which the animation started to play. The other field timeLocation says whether this timestamp is relative to the server's clock or the client's clock.
*/
export type ClockState =
    | { Paused: { animationTimeSecs: number } }
    | { Playing: { localStartMs: number, timeLocation: ObjectLocation } }

export type InterpolationStyle = "Step" | "Linear" | "EaseInOut" | "EaseIn" | "EaseOut"

export type WeightTransition = {
    /** The simulation time in ms at which the weight transition started in timeLocation. */
    startedAtMs: number
    /** Whether startedAtMs is client time or estimated server time. */
    timeLocation: ObjectLocation
    initValue: number
    finalValue: number
    durationMs: number
    easing: InterpolationStyle
}

/** A clock is a helper object that says how far along the animation is in a SkAnimator, PropertyAnimator, or SoundPlayer.
  */
export type ClockInfo = {
    /** Whether it is paused or playing, and how far along it is. */
    clockState: ClockState
    /** Whether it should loop when done. */
    shouldLoop: boolean
    /** A multiplier onto the normal speed. */
    speed: number
}

export type BallPrimitiveSettings = {
    radius: number,
}

export type CylinderPrimitiveSettings = {
    radius: number,
    length: number,
}

export type BrickPrimitiveSettings = {
    size: Vec3,
}

/** Tile size is in world units, and tile offset is fractions of a tile. The origin for the tiling is the -Z top pole. */
export type BallMaterialSettings = {
    /** The objectId of the Material to use. */
    materialInfoId: ObjectId
    tileSizeU: number
    tileSizeV: number
    tileOffsetU: number
    tileOffsetV: number
}

/** Tile size is in world units, and tile offset is fractions of a tile. The origin for the tiling is the +Z end of the -X side, and the top left corner of the square around each cap. */
export type CylinderMaterialSettings = {
    /** The objectId of the Material to use. */
    materialInfoId: ObjectId
    tileSizeU: number
    tileSizeV: number
    tileOffsetU: number
    tileOffsetV: number
}

/** Tile size is in world units, and tile offset is fractions of a tile. The origin for the tiling is the top left corner (the top face's -X -Z corner and the bottom face's -X +Z corner). */
export type BrickMaterialSettings = {
    /** The objectId of the Material to use. */
    materialInfoId: ObjectId
    tileSizeU: number
    tileSizeV: number
    tileOffsetU: number
    tileOffsetV: number
}

export type MeshMaterialSettings = {
    /** The objectId of the Material to use. */
    materialInfoId: ObjectId
}

/** A ball in the world. */
export type BallInfo = {
    /** Visual Material texture settings. */
    material: BallMaterialSettings | null,
}
    & VisualSettings // <-- DIFFERENT: spread
    & BallPrimitiveSettings // <-- DIFFERENT: PrimitiveInfo is added here and is not on rust's BallInfo
    & PhysicsInfo3D // <-- DIFFERENT: PhysicsInfo3D is added here and is not on rust's BallInfo
    & PhysicsToggles3D // <-- DIFFERENT: PhysicsToggles3D is added here and is not on rust's BallInfo


/** A cylinder in the world. Its symmetry axis is local Z. */
export type CylinderInfo = {
    /** Visual Material texture settings. */
    material: CylinderMaterialSettings | null,
}
    & VisualSettings // <-- DIFFERENT: spread
    & CylinderPrimitiveSettings // <-- DIFFERENT: PrimitiveInfo is added here and is not on rust's CylinderInfo
    & PhysicsInfo3D // <-- DIFFERENT: PhysicsInfo3D is added here and is not on rust's CylinderInfo
    & PhysicsToggles3D // <-- DIFFERENT: PhysicsToggles3D is added here and is not on rust's CylinderInfo


/** A brick in the world. */
export type BrickInfo = {
    /** Visual Material texture settings. */
    material: BrickMaterialSettings | null,
}
    & VisualSettings // <-- DIFFERENT: spread
    & BrickPrimitiveSettings // <-- DIFFERENT: PrimitiveInfo is added here and is not on rust's BrickInfo
    & PhysicsInfo3D // <-- DIFFERENT: PhysicsInfo3D is added here and is not on rust's BrickInfo
    & PhysicsToggles3D // <-- DIFFERENT: PhysicsToggles3D is added here and is not on rust's BrickInfo



/** Use this as a Folder. There is no special behavior, it is simply for organization. */
export type LocalGroupInfo = {
}

/** 
* This is the root of a skeleton. Skeletons only apply to objects with the `followSkeletonId` field.
* 
* This should contain a child SkJoint for each joint in the skeleton. Hierarchies are allowed.
* 
* You can (1) animate it and (2) glue objects to it, using SkAnimation, SkAnimator, and SkAttachment.
* */
export type SkeletonInfo = {
}

/** 
 * A child of Skeleton or SkJoint.
 * 
 * */
export type SkJointInfo = {
    /** This joint's resting location relative to its parent joint. 
     * You can change this to move joints around. SkKeyframePose is combined with this pose, so you can change the orientation of a joint during an animation.
     * */
    restPose: Location3D
}

/** This is how you define an animation.
 * 
 * It should contain a keyframe SkKeyframe child for each timestep in your animation.
 * 
 */
export type SkAnimationInfo = {
    duration: number
    /** The interpolation style of the animation. Use Step for a stop-motion effect. */
    interpolationStyle: InterpolationStyle
}



/** A child of SkAnimation. 
 * 
 * For each desired joint in this keyframe, it should contain a SkKeyframePose child.
 * 
*/
export type SkKeyframeInfo = {
    /** Time in seconds in the animation. */
    time: number
}

/** A child of SkKeyframe.
 *
 * The pose of an individual joint.
 */
export type SkKeyframePoseInfo = {
    jointName: string
    /** This joint's location relative to its restPose.
     * A zero position and rotation leave the joint at its rest pose. 
     */
    localPose: Location3D
}

/** Use this to glue an object in a RigidGroup to an SkJoint. 
 * 
*/
export type SkAttachmentInfo = {
    /** The objectId that should follow the SkJoint. */
    objectId: ObjectId | null
    jointName: string

    /** The offset the object will have relative to the joint even as the joint moves. */
    offsetFromJoint: Location3D
}

/** 
 * This only applies to Material objects whose albedoTextureDataId have transparency. Default is Opaque. 
 * 
 * - Opaque means to render the underlying object as solid, meaning show the base color in the transparent areas.
 * - Mask makes the underlying object either opaque or invisible in areas, based on the transparency cutoff.
 * - Blend uses the texture alpha multiplied by the object opacity for smooth transparency.
*/
export type AlphaMode = "Opaque" | { Mask: { cutoff: number } } | "Blend"

/** An image or "texture" that is shown on an object. Albedo is the main texture. The other fields are optional PBR properties. */
export type MaterialInfo = {
    /** This is the main texture for color. Four values (RGBA) per pixel. */
    albedoTextureDataId: DataId | null
    /** Optional metallic information. One value per pixel. */
    metallicTextureDataId: DataId | null
    /** Optional roughness information. One value per pixel. */
    roughnessTextureDataId: DataId | null
    /** Optional normal information. Three values (XYZ) per pixel. */
    normalTextureDataId: DataId | null
    /** Says how to handle the underlying object's transparency. Only relevant when albedoTextureDataId has transparency in it. */
    alphaMode: AlphaMode
    /** Render both sides of the surface, with back-face normals reversed for lighting. */
    isDoubleSided: boolean
}




/** A RigidGroup is a bunch of objects that move together. All Ball, Brick, and Cylinder child descendants of this are glued together and will move as one rigid body.
 * 
 * If you want to add animations, you must set followSkeletonId, use a SkAnimator to animate it, and add SkAttachment objects to glue objects to the SkJoints. There are some AI tools to automate this. */
export type RigidGroupInfo = {
    /** The objectId of the Skeleton that this rigid group will follow. */
    followSkeletonId: ObjectId | null
} & PhysicsInfo3D // <-- DIFFERENT: PhysicsInfo3D is added here and is not on rust's RigidGroupInfo


/** 
 * A mesh in the world.
 * 
 * To animate this mesh visually, you must set followSkeletonId and use a SkAnimator to animate it. */
export type MeshInfo = {
    scale: Vec3,
    meshDataId: DataId,
    /** The objectId of the Skeleton that this mesh will visually follow. */
    followSkeletonId: ObjectId | null,
    /** The objectId of the Ball, Brick, Cylinder, or RigidGroup that the mesh will use as a collider and visually follow. */
    physicalObjectId: ObjectId | null,
    /** Visual Material texture settings. */
    material: MeshMaterialSettings | null,
} & VisualSettings // <-- DIFFERENT: spread




/**
 * This is how you render text. All text must be attached to an object.
 *
 * All measurements are in terms of world units, ie 1 block tall.
 * */
export type LabelInfo = {
    /** Emojis are not allowed in label text. */
    text: string,
    /** Required. The objectId of the Ball, Brick, Cylinder, or RigidGroup the label follows. */
    physicalObjectId: ObjectId | null,
    lineHeight: number,
    /** Null means it only breaks on newlines. */
    maxWidth: number | null,
    /** Only applies when there are multiple rows. */
    horizontalAlign: 'left' | 'center' | 'right',
    /** Null means transparent bg. */
    backgroundColor: Color3 | null, 
    backgroundOpacity: number,
    /** Additional offset from the object. */
    offsetFromObject: Vec3,
    /** Additional rotation from the object. */
    rotationFromObject: Vec3,
    /** Whether to swivel to face the camera. */
    billboardMode: 'none' | 'full',
    /** If true, the label does not shrink with distance, and remains the size as if viewed from 10 units away. */
    isConstantSize: boolean,
} & VisualSettings // <-- DIFFERENT: spread

/**
 * A picture in the world. It follows an object and orients itself exactly like a Label.
 *
 * All measurements are in terms of world units, ie 1 block tall.
 * */
export type LabelImageInfo = {
    textureDataId: DataId,
    /** The picture stretches to fill width by height. */
    width: number,
    height: number,
    /** Required. The objectId of the Ball, Brick, Cylinder, or RigidGroup the image follows. */
    physicalObjectId: ObjectId | null,
    /** Additional offset from the object. */
    offsetFromObject: Vec3,
    /** Additional rotation from the object. With none, it faces the object's +Z side. */
    rotationFromObject: Vec3,
    /** Whether to swivel to face the camera. */
    billboardMode: 'none' | 'full',
    /** If true, the image does not shrink with distance, and remains the size as if viewed from 10 units away. */
    isConstantSize: boolean,
} & VisualSettings // <-- DIFFERENT: spread




/** You can never create or delete a Player object yourself. It is automatically created and deleted when a player joins and leaves. It is important for you to set the head and character, or the player won't be able to see or move. 
 * 
*/
export type PlayerInfo = {
    username: string
    /** This can be null in rare cases (e.g., guest players when playtesting). Use the player object's id for a perfect identifer, not this. */
    userid: UserId | null
    /** The objectId of the Ball, Brick, Cylinder, or RigidGroup the player's camera will follow. */
    headObjectId: ObjectId | null
    /** The objectId of the Ball, Brick, Cylinder, or RigidGroup this player controls. */
    characterObjectId: ObjectId | null
    
    /** The color this player picked on their profile, or null. Read when they join, and refreshed by `game.playerService.refreshPlayerProfile()`. */
    preferredColor: Color3 | null
    /** The catalog item this player is wearing on their profile, like a hat, or null. Read when they join, and refreshed by `game.playerService.refreshPlayerProfile()`. */
    equippedItemId: number | null
}


/** Every Script is either a Server or Client script. 
 * - Client scripts run on every client. 
 * - Server scripts run on the server.
 * - Some APIs are only exposed to one or the other; read the docs. 
 * - Deleting a Script stops it. Its listeners, timers, `tick()` loops and GUI stop, and its exports can no longer be called.
 * 
 * Import/Export:
 * - It is valid to write `import { functionName } from './scriptName'`. Hiererarchies of object names are valid.
 * - Script names, paths, & contents are only read once when the script is created.
 *
 * Server/Client communication:
 * - Write `'use server'` at the top of a Server script to make all exports importable by Client scripts.
 * - Write `'use client'` at the top of a Client script to make all exports importable by Server scripts.
 * - The first parameter to all client functions *must* be the player's object id (`player.id`).
 * - Exports must be async functions.
 * - Exports must be validly callable by the other side.
 * - You should validate server function inputs and client function returns, as these are untrusted.
 * 
 * UI:
 * - We expose React-like syntax to draw UI on the screen. Every Tailwind CSS v4 class works, including arbitrary values like `w-[42%]`, and text defaults to black and 16px as on any web page.
 * - You may only use `<div>`, `<button>`, text, arrays, null, className, onClick, onRightClick, onMouseDown, onMouseUp, and onHover primitive props.
 * - Use `<div>` for layout and text. Pointer input on a div reaches both the div (its hover styles and onHover) and the game, unless the div has onClick, onRightClick, onMouseDown or onMouseUp.
 * - Use `<button>` with onClick for clickable UI. A button, or a div with any of those four handlers, keeps presses to itself, so pressing it does not press the game. The innermost element with an onClick handles the click.
 * - onClick is only for the left button. onRightClick fires when the right button goes down and comes back up on the element. onMouseDown and onMouseUp get `"left"` or `"right"`. onMouseUp fires on the element the button is let go over, even if it went down somewhere else. Each one goes to the innermost element that has it.
 * - Style with `className` only. Do not use style props or unsupported HTML tags.
 * - Each shown GUI covers the whole screen, so `w-full` and `h-full` mean the whole screen. Place your content with Tailwind, like `absolute top-4 right-4` for a corner, `absolute bottom-6 left-1/2 -translate-x-1/2` for the bottom middle, or `absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2` for the center. Without that it sits in the top left corner. If you use screen-filling wrappers, you can use `pointer-events-none` to keep them from blocking other GUIs that mount, and `pointer-events-auto` on the content inside.
 * - Build a meter bar as a fixed-size track with a percent-width fill inside it, never as a row of recolored squares.
 * - Round dynamic tailwind classes to whole numbers, because new class strings rebuild the stylesheet.
 *   ```tsx
 *   const HealthMeter = ({ fraction }: { fraction: number }) => {
 *     const percent = Math.round(Math.min(1, Math.max(0, fraction)) * 100)
 *     return <div className="w-48 h-4 rounded-sm bg-black/50 p-1 overflow-hidden">
 *       <div className={`h-full rounded-sm bg-emerald-400 w-[${percent}%]`} />
 *     </div>
 *   }
 *   ```
 * - `useState()` works like usual.
 * - `createState()` works for state that comes from outside react, eg for 'use client' functions. This is similar to useSyncExternalStore.
 *   - Write `createState(initialValue)`. The `.get()` function automatically subscribes the relevant React components to re-render when `.set()` is called. `.set()` is intended to be defined inside a `'use client'` function and called from the server.
 * - Update UI that moves (meters, timers, charge bars) frequently, and use `transition-[width] duration-[Nms] ease-linear` with N about the time between updates (below we use 17ms or 1 tick), so the bar slides between values.
 *   ```tsx
 *   const now = createState(getTime())
 *   const runClock = async () => {
 *     while (true) {
 *       await tick()
 *       now.set(getTime())
 *     }
 *   }
 *   runClock()
 *
 *   const ChargeMeter = ({ chargeStartTime }: { chargeStartTime: number }) => {
 *     const percent = Math.round(Math.min(1, (now.get() - chargeStartTime) / 2000) * 100)
 *     return <div className="w-48 h-4 bg-black/50 overflow-hidden">
 *       <div className={`h-full bg-emerald-400 transition-[width] duration-[17ms] ease-linear w-[${percent}%]`} />
 *     </div>
 *   }
 *   ```
 * - Avoid updating UI from `setInterval`; it makes bars jump.
 * - Do not put UI in the upper left portion of the screen; that is reserved for chat.
 * 
 *
 * Libraries:
 * - You can use `setTimeout()` and `setInterval()`. Note that timers wait for the next tick to run, so they essentially only run on multiples of 16.6ms. Eg `setTimeout(fn, 30)` runs after ~33ms.
 * - You can use `console`, `URL`, `TextEncoder`/`TextDecoder`, `structuredClone`, `atob`/`btoa`, `Event`/`EventTarget`, and `AbortController`.
 * - There is no `fetch`, no `WebSocket`, no storage, and no `crypto` (use `Math.random()`).
 * 
 * Managing Objects:
 *   - Only the server can create and modify objects.
 *   - All objects in the world are replicated to all users. You cannot create an object on just one client.
 */
export type ScriptInfo = {
    objectLocation: ObjectLocation
    // note - rawCode is not here. it's managed by world. this is not a difference, but it's potentially confusing.
}



export type SoundPlayerInfo = {
    /** The sound to play. */
    soundDataId: DataId,
    /** The Clock that lets you pause/play it. */
    clockId: ObjectId | null,
    /** 0-1 */
    volume: number,
}


/**
 *
 * This animates a Skeleton. 
 * 
 * You can have many animators on the same skeleton. They will be blended in order of decreasing priority until each item's weight reaches a total of 100. See also: AnimationService.
 *
 * Start and stop an animation with `.play()` and `.pause()` on this animator's clock, and fade it in and out with `.smoothlyChangeWeight()`.
 *
 * */
export type SkAnimatorInfo = {
    /** The Skeleton to animate. */
    skeletonId: ObjectId | null,
    /** Using this SkAnimation. */
    skAnimationId: ObjectId | null,
    /** The Clock that lets you pause/play it. */
    clockId: ObjectId | null,
    /** 0-10. */
    priority: number,
    /** 0-100. */
    weight: number,
    weightTransition: WeightTransition | null,
}

/** This defines the animation for a PropertyAnimator to play.
 *
 * It should contain a keyframe PropertyKeyframe child for each timestep in your animation.
 * */
export type PropertyAnimationInfo = {
    duration: number
    interpolationStyle: InterpolationStyle
}

/** A child of PropertyAnimation. */
export type PropertyKeyframeInfo = {
    time: number
    value: number
}

/**
 * Use this to smoothly animate one property on any object. 
 * 
 * You can have multiple of these at once. They blend independently given a targetObjectId and property, so you can animate multiple fields at once.
 * 
 * - You can't use this to physically change subitems in a RigidGroup. Use a SkAnimator for that instead.
 * - If you're using this to create a moving platform, be sure it isn't pushable, or else it purposefully won't work - too confusing.
 * - It writes the animated value into the real field every step.
 * 
 */
export type PropertyAnimatorInfo = {
    targetObjectId: ObjectId | null,
    /** Path to the continuous field to animate, for example ["color", "r"], ["opacity"], or ["location", "position", "x"]. */
    propertyPath: string[],
    /** The PropertyAnimation to play. */
    propertyAnimationId: ObjectId | null,
    /** The Clock that lets you pause/play it. */
    clockId: ObjectId | null,
    /** 0-10. */
    priority: number,
    /** 0-100. */
    weight: number,
    weightTransition: WeightTransition | null,
}

/**
 * This smoothly changes a value on an object. Typically you should create this with an `animationService.tweenTo()`.
 *
 * This is equivalent to a PropertyAnimator with 2 keyframes. It is an easier, cheaper way to dynamically move a property if you don't know the values you want beforehand. 
 *
 * - All tween objects are automatically applied to their objects. 
 * - `tweenTo()` creates a tween for you and deletes it once it finishes. 
 * - They write the animated value into the real field every step.
 */
export type TweenInfo = {
    targetObjectId: ObjectId | null,
    /** Path to the number on the target object, for example ["location", "rotation", "y"]. */
    propertyPath: string[],
    fromValue: number,
    toValue: number,
    /** In seconds. */
    duration: number,
    interpolationStyle: InterpolationStyle,
    /** Playing from when the tween started. */
    clockState: ClockState,
}

/**
 * This lets you specify a center point that this object will rotate/scale relative to. The pivot moves, turns and scales with the object. Max one per object.
 *
 * This only applies when using a PropertyAnimator on rotation, or one of the functions on objectService.
 * It is ignored when manually setting rotation/size directly.
 *
 */
export type PivotInfo = {
    physicalObjectId: ObjectId | null,
    offsetFromObject: Vec3,
}


/** Controls the sun location and sky settings.  */
export type WorldLightingInfo = {
    sunOrientation: Orientation3D,
    sunColor: Color3,
    sunIntensity: number,
    /** How soft sun shadows are: how far every shadow edge blurs, in world units. 0 gives hard shadows. */
    sunShadowSoftness: number,
    ambientTopColor: Color3,
    ambientBottomColor: Color3,
    ambientIntensity: number,
    /** 0-1. How dark ambient occlusion can make creases and the ground right next to objects. 0 turns it off, 1 lets the deepest creases go black. */
    ambientOcclusionIntensity: number,
    /** How far ambient occlusion reaches onto nearby surfaces, in world units. */
    ambientOcclusionRadius: number,
    /** 0-1. How much ambient occlusion also darkens sunlight. 0 darkens only the sky light. */
    ambientOcclusionSunlightStrength: number,
    /** The gradient sky color straight overhead. */
    skyTopColor: Color3,
    /** The gradient sky color at the horizon, which both halves of the gradient start from. */
    skyHorizonColor: Color3,
    /** The gradient sky color straight down, below the horizon. */
    skyBottomColor: Color3,
    /** An equirectangular skybox TextureData id. Transparent pixels reveal the gradient sky. */
    skyboxTextureDataId: DataId | null,
    /** 0 means the skybox texture is hidden. 1 means the skybox texture is fully visible. */
    skyboxTextureOpacity: number,
    /** 0-1. How strongly shiny and metal surfaces reflect the sky. 0 turns sky reflections off. */
    skyReflectionStrength: number,
    /** 0-1. Pushes dull colors further from gray, and already vivid ones barely at all. 0 turns it off. */
    vibrance: number,
}








/** This is a helper type. It contains all the fields of either a Ball, Brick, Clock, Cylinder, Group, SkJoint, SkAnimation, SkAnimator, SkAttachment, SkKeyframe, SkKeyframePose, Material, Mesh, Pivot, Player, PropertyAnimation, PropertyAnimator, PropertyKeyframe, RigidGroup, Script, Skeleton, SoundPlayer, Tween, or WorldLighting,
 * plus a `type` field saying which one it is.
 * */
export type ObjectInfo = (
    | ({ type: 'ball' } & BallInfo)
    | ({ type: 'cylinder' } & CylinderInfo)
    | ({ type: 'skJoint' } & SkJointInfo)
    | ({ type: 'skAnimation' } & SkAnimationInfo)
    | ({ type: 'skAttachment' } & SkAttachmentInfo)
    | ({ type: 'skKeyframe' } & SkKeyframeInfo)
    | ({ type: 'skKeyframePose' } & SkKeyframePoseInfo)
    | ({ type: 'skeleton' } & SkeletonInfo)
    | ({ type: 'brick' } & BrickInfo)
    | ({ type: 'clock' } & ClockInfo)
    | ({ type: 'group' } & LocalGroupInfo)
    | ({ type: 'label' } & LabelInfo)
    | ({ type: 'labelImage' } & LabelImageInfo)
    | ({ type: 'material' } & MaterialInfo)
    | ({ type: 'worldLighting' } & WorldLightingInfo)
    | ({ type: 'rigidGroup' } & RigidGroupInfo)
    | ({ type: 'player' } & PlayerInfo)
    | ({ type: 'mesh' } & MeshInfo)
    | ({ type: 'pivot' } & PivotInfo)
    | ({ type: 'script' } & ScriptInfo)
    | ({ type: 'skAnimator' } & SkAnimatorInfo)
    | ({ type: 'propertyAnimation' } & PropertyAnimationInfo)
    | ({ type: 'propertyKeyframe' } & PropertyKeyframeInfo)
    | ({ type: 'propertyAnimator' } & PropertyAnimatorInfo)
    | ({ type: 'tween' } & TweenInfo)
    | ({ type: 'soundPlayer' } & SoundPlayerInfo)
)

export type RustObjectSnapshot = {
    readonly id: ObjectId,
    name: string,
    parent: ObjectId | null, // <-- DIFFERENT: read from the world hierarchy
} & ObjectInfo // <-- DIFFERENT: spread
