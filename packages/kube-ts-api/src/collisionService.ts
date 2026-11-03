import { ourGlobals } from "./object_types/__globalOps__";
import { removeFnFromArray } from "./lib/removeFnFromArray";
import type { LiveObject } from "./world";

export type CollisionCallback = (object1: LiveObject, object2: LiveObject) => void;

/**
 * - collisionService fires even when isWalkthrough=true.
 * - It never fires between two objects that both have isWalkthrough=true.
 * - For RigidGroups, collisionService fires using the subitem(s) involved in the collision.
 *
 * If you want to detect collisions accurately, use this service on the client, and call a 'use server' function that confirms the hit.
 * On the server, collision detection of characters can be inaccurate.
 */
export type CollisionService = {
  /** Only fires once, when the collision starts. It does not keep firing while the objects stay in contact. Objects that are already touching when the world loads, which on a client is when the player joins, don't fire it, but they still fire the stop listener when they separate. */
  addCollisionListener: (callback: CollisionCallback) => void;
  /** */
  removeCollisionListener: (callback: CollisionCallback) => void;
  /** */
  addCollisionStopListener: (callback: CollisionCallback) => void;
  /** */
  removeCollisionStopListener: (callback: CollisionCallback) => void;
};

export const collisionService: CollisionService = {
  addCollisionListener: (callback: CollisionCallback) => {
    ourGlobals.__onCollisionCallbacks__.push(callback);
  },
  removeCollisionListener: (callback: CollisionCallback) => {
    removeFnFromArray(ourGlobals.__onCollisionCallbacks__, callback);
  },
  addCollisionStopListener: (callback: CollisionCallback) => {
    ourGlobals.__onCollisionStopCallbacks__.push(callback);
  },
  removeCollisionStopListener: (callback: CollisionCallback) => {
    removeFnFromArray(ourGlobals.__onCollisionStopCallbacks__, callback);
  },
};
