import { denoOps, ourGlobals, type RustCastHit } from "./object_types/__globalOps__";
import type { ObjectId, Vec3 } from "./object_types/shared";
import type { ObjectInfo } from "./object_types/types";
import { getRustObject, type LiveObject } from "./world";


// the ObjectType parameter narrows the callback to the filtered type, e.g. a "player" filter receives LiveObject & { type: "player" }
export type ObjectCreatedCallback<ObjectType extends ObjectInfo["type"]> = (object: LiveObject & { type: ObjectType }) => void;
// deleted objects are already gone from the world, so the callback gets the id and type instead of a live object
export type ObjectDeletedCallback<ObjectType extends ObjectInfo["type"]> = (objectId: ObjectId, objectType: ObjectType) => void;

// callbacks are bucketed by their type filter so dispatch only touches the matching bucket; unfiltered callbacks live under the "undefined" key
export type ObjectCallbacksOfType<CallbackType> = { [objectTypeKey: string]: CallbackType[] | undefined };

const pushCallback = <CallbackType>(callbacksOfType: ObjectCallbacksOfType<CallbackType>, objectType: ObjectInfo["type"] | undefined, callback: CallbackType) => {
    const callbacks = callbacksOfType[`${objectType}`] ?? (callbacksOfType[`${objectType}`] = []);
    callbacks.push(callback);
};

const removeCallback = <CallbackType>(callbacksOfType: ObjectCallbacksOfType<CallbackType>, callback: CallbackType) => {
    for (const callbacks of Object.values(callbacksOfType)) {
        if (callbacks === undefined) continue;
        const callbackIndex = callbacks.indexOf(callback);
        if (callbackIndex === -1) continue;
        callbacks.splice(callbackIndex, 1);
        return;
    }
};

// Each catalog item is downloaded once per game, so adding it again (like a player's hat on every respawn) doesn't wait on a download.
// A catalog item edited on the website keeps its old look until the game restarts.
const catalogItemJsonOfId: { [catalogItemId: number]: string } = {};


/** What `castRay`, `castObjectShape`, and `getGroundSupport` hit. */
export type RaycastHit = {
    /** The Ball, Brick, or Cylinder that was hit. For a RigidGroup, this is the subitem that was hit. */
    object: LiveObject,
    /** How far the ray or shape traveled before it hit. */
    distance: number,
    /** World-space point where it hit. */
    position: Vec3,
    /** Unit vector pointing out of the surface that was hit. */
    normal: Vec3,
};

const raycastHitOfRustCastHit = (rustCastHit: RustCastHit | null): RaycastHit | null => {
    if (rustCastHit === null) return null;
    const object = getRustObject(rustCastHit.objectId);
    if (object === null) return null;
    return { object, distance: rustCastHit.distance, position: rustCastHit.position, normal: rustCastHit.normal };
};

export type RaycastOptions = {
    /** Stop looking past this distance. Defaults to unlimited. */
    maxDistance?: number,
    /** Whether isWalkthrough objects can be hit. Defaults to false. */
    includeWalkthrough?: boolean,
    /** Objects the ray should pass through. Excluding a RigidGroup excludes all of its subitems, so pass a character's RigidGroup id here to not hit yourself. */
    excludeObjectIds?: ObjectId[],
};

export type ShapeCastOptions = RaycastOptions & {
    /** Defaults to no offset. */
    startOffset?: Vec3,
};

export type GroundSupportOptions = {
    /** Defaults to 0.01. */
    maxDistance?: number,
    /** Defaults to 55. */
    maxGroundSlopeDegrees?: number,
};


/** 
 * This service exposes methods for reading and modifying objects.
 * 
 * All of the functions here are recursive: `scale`, `rotate`, `move`, `setObjectWalkthrough`, etc., meaning they apply to the object and all of its children.
 * 
 * If you only want to operate on one object and not its children, simply write `object.propery = value`, and don't use the functions here.
 * 
 * If you call these in a Client script the changes are only applied locally (not broadcast).
 */
export type ObjectService = {
    /** Returns the object with this ObjectId. */
    getObjectById: (objectId: ObjectId) => LiveObject | null;
    /** Returns any object with this name. */
    getObjectByName: (name: string) => LiveObject | null;
    /** Returns all the objects with this name. */
    getObjectsByName: (name: string) => LiveObject[];
    /** Returns all the objects with this type. */
    getObjectsByType: (objectType: ObjectInfo["type"]) => LiveObject[];

    createObject: (name: string, objectInfo: ObjectInfo, parentId: ObjectId) => LiveObject | null;
    deleteObject: (objectId: ObjectId) => void;
    /** Remember to move the clone afterwards so it doesn't overlap. */
    cloneObject: (objectId: ObjectId, cloneName: string) => LiveObject;
    createSkAttachmentsInRigidGroup: (skeletonId: ObjectId, rigidGroupId: ObjectId) => LiveObject[];
    /**
     * Adds a catalog item (hat) to the world by its id.
     * Only callable by a Server Script.
     */
    addCatalogItemToWorld: (catalogItemId: number, parentId: ObjectId) => Promise<LiveObject | null>;

    /** Scales the object by the multiplier. References Pivot if there is one. Note that repeatedly calling this will grow/shrink the object multiple times. */
    scaleObjectBy: (objectId: ObjectId, scaleMultiplier: number) => void;
    /** Rotate the object around the provided `axis` in world coordinates. References Pivot if there is one. */
    rotateObjectBy: (objectId: ObjectId, axis: Vec3, degrees: number) => void;
    /** Rotate the object to the provided rotation, in degrees like `location.rotation`. References Pivot if there is one. */
    rotateObjectTo: (objectId: ObjectId, rotation: Vec3) => void;
    moveObjectBy: (objectId: ObjectId, offset: Vec3) => void;
    moveObjectTo: (objectId: ObjectId, position: Vec3) => void;
    /** 
    This is the only way you should teleport things. This is `moveObjectTo`, but without interpolating/physically sweeping the object. */
    teleportObjectTo: (objectId: ObjectId, position: Vec3) => void;

    setObjectWalkthrough: (objectId: ObjectId, isWalkthrough: boolean) => void;
    setObjectPushable: (objectId: ObjectId, isPushable: boolean) => void;
    setObjectDensity: (objectId: ObjectId, density: number) => void;

    /** Casts a ray through the physics world and returns the closest hit, or null. The direction does not need to be normalized.
    */
    castRay: (rayOrigin: Vec3, rayDirection: Vec3, options?: RaycastOptions) => RaycastHit | null;

    /** Casts the object and returns the closest hit, or null. The direction does not need to be normalized. */
    castObjectShape: (objectId: ObjectId, direction: Vec3, options?: ShapeCastOptions) => RaycastHit | null;

    /** What the object is standing on, or null. Use this to check if a character is grounded. */
    getGroundSupport: (objectId: ObjectId, options?: GroundSupportOptions) => RaycastHit | null;

    /** Shortcut that uses `getGroundSupport`. If there is no ground, this is just the regular velocity. */
    getVelocityRelativeToGround: (objectId: ObjectId, options?: GroundSupportOptions) => Vec3;

    /** The world's gravity, in distance per second squared. */
    getGravity: () => Vec3;

    /** Get the x, y, z basis vectors (axes) of the object, based on its current rotation. */
    getObjectAxes: (objectId: ObjectId) => { x: Vec3, y: Vec3, z: Vec3 };

    /** Fires when an object is created. Does not fire for objects that already existed when the world loaded. 
     */
    addObjectCreatedListener: <ObjectType extends ObjectInfo["type"]>(
        callback: ObjectCreatedCallback<ObjectType>,
        objectType?: ObjectType,
    ) => void;
    /** */
    removeObjectCreatedListener: <ObjectType extends ObjectInfo["type"]>(callback: ObjectCreatedCallback<ObjectType>) => void;

    /** Fires when an object is deleted. 
     * Deleting an object fires this for it and every descendant. */
    addObjectDeletedListener: <ObjectType extends ObjectInfo["type"]>(
        callback: ObjectDeletedCallback<ObjectType>,
        objectType?: ObjectType,
    ) => void;
    /** */
    removeObjectDeletedListener: <ObjectType extends ObjectInfo["type"]>(callback: ObjectDeletedCallback<ObjectType>) => void;
};

const convertObjectIdsToObjects = (objectIds: ObjectId[]): LiveObject[] => {
    const liveObjects: LiveObject[] = [];
    for (const objectId of objectIds) {
        const liveObject = getRustObject(objectId);
        if (liveObject === null) continue;
        liveObjects.push(liveObject);
    }
    return liveObjects;
};

export const objectService: ObjectService = {
    getObjectById: (objectId: ObjectId) => {
        return getRustObject(objectId);
    },
    getObjectByName: (name: string): LiveObject | null => {
        const objects = objectService.getObjectsByName(name);
        return objects[0] ?? null;
    },
    getObjectsByName: (name: string): LiveObject[] => {
        const objectIds = denoOps.__getRustObjectIdsByName__(name);
        return convertObjectIdsToObjects(objectIds);
    },
    getObjectsByType: (objectType: ObjectInfo["type"]): LiveObject[] => {
        const objectIds = denoOps.__getRustObjectIdsByType__(objectType);
        return convertObjectIdsToObjects(objectIds);
    },

    createObject: (name: string, objectInfo: ObjectInfo, parentId: ObjectId) => {
        const objectId = denoOps.__createRustObj__(name, JSON.stringify(objectInfo), parentId);
        return getRustObject(objectId);
    },
    deleteObject: (objectId: ObjectId) => {
        denoOps.__deleteRustObj__(objectId);
    },
    addCatalogItemToWorld: async (catalogItemId: number, parentId: ObjectId) => {
        const catalogItemJson = catalogItemJsonOfId[catalogItemId] ?? await denoOps.__fetchCatalogItemJson__(catalogItemId);
        catalogItemJsonOfId[catalogItemId] = catalogItemJson;
        const objectId = denoOps.__addCatalogItemToWorld__(catalogItemJson, parentId);
        return objectId === null ? null : getRustObject(objectId);
    },
    cloneObject: (objectId: ObjectId, cloneName: string) => {
        const clonedObjectId = denoOps.__cloneRustObj__(objectId, cloneName);
        let clonedObject = getRustObject(clonedObjectId);
        if (clonedObject === null) {
            throw new Error(`Cloned object with id ${clonedObjectId} was not found`);
        }
        return clonedObject;
    },
    createSkAttachmentsInRigidGroup: (skeletonId: ObjectId, rigidGroupId: ObjectId) => {
        const skAttachmentIds = denoOps.__createSkAttachmentsInRigidGroup__(skeletonId, rigidGroupId);
        return convertObjectIdsToObjects(skAttachmentIds);
    },

    // these all act recursively on all children
    scaleObjectBy: (objectId: ObjectId, scaleMultiplier: number) => {
        denoOps.__scaleRustObjects__(objectId, scaleMultiplier);
    },
    rotateObjectBy: (objectId: ObjectId, axis: Vec3, degrees: number) => {
        denoOps.__rotateRustObjects__(objectId, JSON.stringify(axis), degrees);
    },
    rotateObjectTo: (objectId: ObjectId, rotation: Vec3) => {
        denoOps.__rotateRustObjectsTo__(objectId, JSON.stringify(rotation));
    },
    moveObjectBy: (objectId: ObjectId, offset: Vec3) => {
        denoOps.__moveRustObjects__(objectId, JSON.stringify(offset));
    },
    moveObjectTo: (objectId: ObjectId, position: Vec3) => {
        denoOps.__moveRustObjectsTo__(objectId, JSON.stringify(position));
    },
    teleportObjectTo: (objectId: ObjectId, position: Vec3) => {
        denoOps.__teleportRustObjectsTo__(objectId, JSON.stringify(position));
    },

    setObjectWalkthrough: (objectId: ObjectId, isWalkthrough: boolean) => {
        denoOps.__setRustObjectsWalkthrough__(objectId, isWalkthrough);
    },
    setObjectPushable: (objectId: ObjectId, isPushable: boolean) => {
        denoOps.__setRustObjectsPushable__(objectId, isPushable);
    },
    setObjectDensity: (objectId: ObjectId, density: number) => {
        denoOps.__setRustObjectsDensity__(objectId, density);
    },

    getVelocityRelativeToGround: (objectId: ObjectId, options?: GroundSupportOptions): Vec3 => {
        return denoOps.__getVelocityRelativeToGround__(objectId, JSON.stringify(options ?? {}));
    },
    getGravity: (): Vec3 => {
        return denoOps.__getGravity__();
    },
    getObjectAxes: (objectId: ObjectId): { x: Vec3, y: Vec3, z: Vec3 } => {
        return denoOps.__getObjectAxes__(objectId);
    },

    castRay: (rayOrigin: Vec3, rayDirection: Vec3, options?: RaycastOptions): RaycastHit | null => {
        return raycastHitOfRustCastHit(denoOps.__castRay__(
            JSON.stringify(rayOrigin),
            JSON.stringify(rayDirection),
            options?.maxDistance ?? Number.MAX_VALUE,
            options?.includeWalkthrough ?? false,
            JSON.stringify(options?.excludeObjectIds ?? []),
        ));
    },
    castObjectShape: (objectId: ObjectId, direction: Vec3, options?: ShapeCastOptions): RaycastHit | null => {
        return raycastHitOfRustCastHit(denoOps.__castObjectShape__(
            objectId,
            JSON.stringify(options?.startOffset ?? { x: 0, y: 0, z: 0 }),
            JSON.stringify(direction),
            options?.maxDistance ?? Number.MAX_VALUE,
            options?.includeWalkthrough ?? false,
            JSON.stringify(options?.excludeObjectIds ?? []),
        ));
    },
    getGroundSupport: (objectId: ObjectId, options?: GroundSupportOptions): RaycastHit | null => {
        return raycastHitOfRustCastHit(denoOps.__getGroundSupport__(objectId, JSON.stringify(options ?? {})));
    },

    // ------------ created ------------
    // the runtime type bucket is what upholds the narrowed callback type, so the stored callback is safely widened
    addObjectCreatedListener: <ObjectType extends ObjectInfo["type"]>(
        callback: ObjectCreatedCallback<ObjectType>,
        objectType?: ObjectType,
    ) => {
        pushCallback(ourGlobals.__onObjectCreatedCallbacks__, objectType, callback as ObjectCreatedCallback<ObjectInfo["type"]>);
    },
    removeObjectCreatedListener: <ObjectType extends ObjectInfo["type"]>(callback: ObjectCreatedCallback<ObjectType>) => {
        removeCallback(ourGlobals.__onObjectCreatedCallbacks__, callback as ObjectCreatedCallback<ObjectInfo["type"]>);
    },

    // ------------ deleted ------------
    addObjectDeletedListener: <ObjectType extends ObjectInfo["type"]>(
        callback: ObjectDeletedCallback<ObjectType>,
        objectType?: ObjectType,
    ) => {
        pushCallback(ourGlobals.__onObjectDeletedCallbacks__, objectType, callback as ObjectDeletedCallback<ObjectInfo["type"]>);
    },
    removeObjectDeletedListener: <ObjectType extends ObjectInfo["type"]>(callback: ObjectDeletedCallback<ObjectType>) => {
        removeCallback(ourGlobals.__onObjectDeletedCallbacks__, callback as ObjectDeletedCallback<ObjectInfo["type"]>);
    },
};
