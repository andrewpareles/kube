import type { CollisionCallback } from "../collisionService";
import type { GameChatCallback } from "../gameChatService";
import type { KeyCallback, MouseButtonCallback } from "../clientInputService";
import type { CharacterStepCallback } from "../clientCharacterService";
import { JsCallbackInputs } from "../jsCallbacks";
import type { ObjectCreatedCallback, ObjectDeletedCallback, ObjectCallbacksOfType } from "../objectService";
import type { GuiHandler, GuiMouseButtonHandler } from "../gui/guiTypes";
import type { Color3, Location3D, ObjectId, Vec3 } from "./shared"
import type { InterpolationStyle, RustObjectSnapshot, ObjectInfo } from "./types"

// -------------- Deno.core.ops is guaranteed to exist when we run the scripts --------------

// castRay, castObjectShape and getGroundSupport all return this. objectService turns the objectId into a live object.
export type RustCastHit = { objectId: ObjectId; distance: number; position: Vec3; normal: Vec3 };

// make sure these types always match the Rust implementations
// Structured engine inputs are JSON strings, so getters run before entering a Rust op.
type DenoOps = {
  // -------------- get/set an object --------------
  // BOTH
  __getRustRootObjectId__: () => ObjectId; // get root
  __getWorldChangeCount__: () => number; // INTERNAL: goes up whenever the world might have changed
  __getRustObject__: (objectId: ObjectId) => RustObjectSnapshot | null; // get
  __getRustChildrenIds__: (objectId: ObjectId) => ObjectId[]; // children
  __getRustObjectIdsByName__: (name: string) => ObjectId[]; // name search
  __getRustObjectIdsByType__: (objectType: ObjectInfo["type"]) => ObjectId[]; // type search

  __setRustObjectField__: (objectId: ObjectId, fieldPath: string, value: string,) => void; // set

  // -------------- playerService.getLocalPlayerObjectId --------------
  // CLIENT
  __getRustLocalPlayerObjectId__: () => ObjectId; // which player this client is
  __setLocalPlayerMetadata__: (metadataJson: string) => void;

  // -------------- playerService.getPlayerMetadata --------------
  // BOTH
  __getPlayerMetadata__: (playerObjectId: ObjectId) => string | null;
  __getPlayerCameraDirection__: (playerObjectId: ObjectId) => Vec3 | null;
  __getPlayerPing__: (playerObjectId: ObjectId) => number | null;

  // -------------- playerService.refreshPlayerProfile --------------
  // SERVER
  __fetchPlayerProfile__: (playerObjectId: ObjectId) => Promise<{ preferredColor: Color3 | null, equippedItemId: number | null }>;

  // -------------- playerService.getServerFnCaller, getCharacterLocationAtTime, getPlayerMetadataAtTime --------------
  // SERVER
  __getServerFnCaller__: () => ObjectId | null;
  __getCharacterLocationAtTime__: (objectId: ObjectId, timeMs: number) => Location3D | null;
  __getPlayerMetadataAtTime__: (playerObjectId: ObjectId, timeMs: number) => string | null;

  // -------------- output --------------
  // BOTH
  __pushConsoleLogString__: (consoleLogString: string, consoleLogLevel: string) => void;

  // -------------- clientInputService camera --------------
  // CLIENT
  __getCameraDirection__: () => Vec3;
  __lockCameraDirection__: (direction: string) => void;
  __unlockCameraDirection__: () => void;
  __setZoomRange__: (minZoom: number, maxZoom: number) => void;
  __getZoom__: () => number;
  __getDesiredZoom__: () => number;
  __setFOV__: (degrees: number | null) => void;
  __setCameraOffset__: (offset: string) => void;
  __setIsMouseLocked__: (isMouseLocked: boolean) => void;
  __setMouseCursor__: (mouseCursor: string) => void;

  // -------------- clientInputService.getMouseRay --------------
  // CLIENT
  __getMouseRay__: () => { origin: Vec3; direction: Vec3 } | null;

  // -------------- objectService.castRay --------------
  // BOTH
  __castRay__: (
    rayOrigin: string,
    rayDirection: string,
    maxDistance: number,
    includeWalkthrough: boolean,
    excludeObjectIds: string,
  ) => RustCastHit | null;

  // -------------- objectService ground and shape casts --------------
  // BOTH
  __castObjectShape__: (
    objectId: ObjectId,
    startOffset: string,
    direction: string,
    maxDistance: number,
    includeWalkthrough: boolean,
    excludeObjectIds: string,
  ) => RustCastHit | null;
  __getGroundSupport__: (objectId: ObjectId, options: string) => RustCastHit | null;
  __getVelocityRelativeToGround__: (objectId: ObjectId, options: string) => Vec3;
  __getGravity__: () => Vec3;

  // -------------- clientCharacterService --------------
  // CLIENT
  __applyCharacterChanges__: (characterChanges: string) => void; // INTERNAL

  // -------------- objectService.getObjectAxes --------------
  // BOTH
  __getObjectAxes__: (objectId: ObjectId) => { x: Vec3; y: Vec3; z: Vec3 };

  // -------------- time --------------
  // BOTH
  __getTimeMs__: () => number; // getTime()
  // CLIENT
  __getDelayedWorldTimeMs__: () => number; // getDelayedWorldTime()

  // -------------- tick --------------
  // BOTH
  __tickInternal__: (scriptId: ObjectId) => Promise<void>; // await tick()
  __stopScriptInternal__: () => Promise<never>; // await stopScript()
  __isFnFromLiveScript__: (fn: (...args: never[]) => unknown) => boolean; // INTERNAL
  __registerJsCallbackFn__: (jsCallbackFn: (jsCallbackInputsList: JsCallbackInputs[]) => void) => void; // INTERNAL

  // -------------- objectService --------------
  // BOTH
  __createRustObj__: (name: string, value: string, parentId: ObjectId,) => ObjectId; // create
  __deleteRustObj__: (objectId: ObjectId) => void; // delete
  __cloneRustObj__: (objectId: ObjectId, cloneName: string) => ObjectId; // clone
  __createSkAttachmentsInRigidGroup__: (skeletonId: ObjectId, rigidGroupId: ObjectId) => ObjectId[];

  __moveRustObjects__: (objectId: ObjectId, offset: string) => void;
  __moveRustObjectsTo__: (objectId: ObjectId, position: string) => void;
  __teleportRustObjectsTo__: (objectId: ObjectId, position: string) => void;
  __scaleRustObjects__: (objectId: ObjectId, scaleMultiplier: number,) => void;
  __rotateRustObjects__: (objectId: ObjectId, axis: string, degrees: number,) => void;
  __rotateRustObjectsTo__: (objectId: ObjectId, rotation: string,) => void;
  __setRustObjectsWalkthrough__: (objectId: ObjectId, isWalkthrough: boolean) => void;
  __setRustObjectsPushable__: (objectId: ObjectId, isPushable: boolean) => void;
  __setRustObjectsDensity__: (objectId: ObjectId, density: number) => void;

  // -------------- objectService.addCatalogItemToWorld --------------
  // SERVER
  __fetchCatalogItemJson__: (catalogItemId: number) => Promise<string>;
  __addCatalogItemToWorld__: (catalogItemJson: string, parentId: ObjectId) => ObjectId | null;

  // -------------- templateService --------------
  // BOTH
  __cutRustObjToTemplate__: (objectId: ObjectId, templateName: string) => void; // cut template
  __pasteRustObjFromTemplate__: (templateName: string, parentId: ObjectId) => ObjectId; // paste template

  // -------------- clockService --------------
  // BOTH
  __pauseClock__: (clockId: ObjectId) => void;
  __playClock__: (clockId: ObjectId, animationTimeSecs: number) => void;
  __setClockSpeed__: (clockId: ObjectId, speed: number) => void;

  // -------------- animationService --------------
  // BOTH
  __smoothlyChangeWeight__: (animatorId: ObjectId, targetWeight: number, durationMs: number, interpolationStyle: InterpolationStyle | null,) => void;
  __tweenTo__: (objectId: ObjectId, propertyPath: string, currentValue: number, targetValue: number, durationMs: number, interpolationStyle: InterpolationStyle | null,) => ObjectId;

  // -------------- server/client functions --------------
  // SERVER
  __registerJsServerFn__: (fnIdentifier: string, serverFn: (args: string) => Promise<string>,) => void; // INTERNAL
  __callJsClientFnFromServer__: (playerObjectId: ObjectId, fnIdentifier: string, jsArgs: string,) => Promise<string>; // INTERNAL

  // CLIENT
  __registerJsClientFn__: (fnIdentifier: string, clientFn: (args: string) => Promise<string>,) => void; // INTERNAL
  __callJsServerFnFromClient__: (fnIdentifier: string, jsArgs: string,) => Promise<string>; // INTERNAL

  // -------------- gui --------------
  // CLIENT
  __setRustGuiRoot__: (scriptObjectId: ObjectId, guiKey: string, guiRoot: string) => void;
  __clearRustGuiRoot__: (scriptObjectId: ObjectId, guiKey: string) => void;
  __registerRustGuiHandler__: (scriptObjectId: ObjectId, guiKey: string, handlerId: string, handler: GuiHandler | GuiMouseButtonHandler) => void;
  __clearRustGuiHandlers__: (scriptObjectId: ObjectId, guiKey: string) => void;

}


// define our own globals which ops can use
type OurGlobals = {
  // -------------- objectService listeners --------------
  // BOTH
  __onObjectCreatedCallbacks__: ObjectCallbacksOfType<ObjectCreatedCallback<ObjectInfo["type"]>>;
  __onObjectDeletedCallbacks__: ObjectCallbacksOfType<ObjectDeletedCallback<ObjectInfo["type"]>>;

  // -------------- collisionService --------------
  // BOTH
  __onCollisionCallbacks__: CollisionCallback[];
  __onCollisionStopCallbacks__: CollisionCallback[];

  // -------------- clientInputService --------------
  // CLIENT
  __onKeyDownCallbacks__: KeyCallback[];
  __onKeyUpCallbacks__: KeyCallback[];
  __onMouseDownCallbacks__: MouseButtonCallback[];
  __onMouseUpCallbacks__: MouseButtonCallback[];
  __pressedKeyNames__: Set<string>;
  __pressedMouseButtonNames__: Set<string>;

  // -------------- clientCharacterService --------------
  // CLIENT
  __onBeforeCharacterStepCallbacks__: CharacterStepCallback[];
  __onAfterCharacterStepCallbacks__: CharacterStepCallback[];

  // -------------- gameChatService --------------
  // SERVER
  __onGameChatMessageCallbacks__: GameChatCallback[];
}

// declare the ops, even though we should really use denoOps and not get the stuff globally, just for clarity
declare global { namespace Deno { namespace core { const ops: DenoOps } } }
// in future could look into adding this as sibling to ops: const unrefOpPromise: <T>(promise: Promise<T>) => void;

const denoOpsUnsafe: DenoOps | undefined = Deno?.core?.ops
if (!denoOpsUnsafe) throw new Error("Deno.core.ops is not available")
export const denoOps: DenoOps = denoOpsUnsafe




const ourGlobals: OurGlobals = {
  __onObjectCreatedCallbacks__: {},
  __onObjectDeletedCallbacks__: {},
  __onCollisionCallbacks__: [],
  __onCollisionStopCallbacks__: [],
  __onKeyDownCallbacks__: [],
  __onKeyUpCallbacks__: [],
  __onMouseDownCallbacks__: [],
  __onMouseUpCallbacks__: [],
  __pressedKeyNames__: new Set(),
  __pressedMouseButtonNames__: new Set(),
  __onBeforeCharacterStepCallbacks__: [],
  __onAfterCharacterStepCallbacks__: [],
  __onGameChatMessageCallbacks__: [],
}

Object.defineProperty(globalThis, "__ourInternalGlobals__", {
  value: ourGlobals,
  enumerable: false,
})
export { ourGlobals }

// declare global {   namespace globalThis {     const __ourInternalGlobals__: OurGlobals;     } }
// ^ this is technically true but clearer if we access it via ourGlobals:
