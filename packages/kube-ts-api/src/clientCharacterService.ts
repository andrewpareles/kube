import { ourGlobals } from "./object_types/__globalOps__";
import { removeFnFromArray } from "./lib/removeFnFromArray";
import type { ObjectId, Vec3 } from "./object_types/shared";

/** */
export type CharacterChanges = {
  /** Delta in velocity. */
  velocityChange?: Vec3,
  /** Delta in angular velocity (degrees). */
  angularVelocityChange?: Vec3,
  /** New velocity. If `velocityChange` is provided, it's added on. */
  velocity?: Vec3,
  /** New position. */
  position?: Vec3,
};

/** A function that returns the changes you want to make to the character. This is called per physics step. You can't `await` here.
 * 
 * To time things across steps, add up `dtSecs`. `getTime()` is also accurate here. 
 * 
 * */
export type CharacterStepCallback = (characterObjectId: ObjectId, dtSecs: number) => CharacterChanges | void;

/**
 * Only callable by a Client Script.
 * */
export type ClientCharacterService = {
  /**
   * This is the preferred way to program a character controller, vehicle controller, etc. It runs on the client, right before each physics step.
   *
   * In order to detect quick key/mouse inputs, you should usually save them in a variable and have this function make relevant changes.
   * You're allowed to use functions like `game.clientInputService.isKeyDown()` here too, but just know they shouldn't be used alone as they won't detect presses that happen faster than a tick.
   *
   * Example usage: `game.clientCharacterService.addBeforeStepListener((characterObjectId, dtSecs) => ({ velocityChange: { x: 0, y: 0, z: 20 * dtSecs } }))`.
   *
  */
  addBeforeStepListener: (callback: CharacterStepCallback) => void,
  removeBeforeStepListener: (callback: CharacterStepCallback) => void,

  /** Similar to `addBeforeStepListener`, but runs right after each physics step, so you can see where the character ended up and correct it, like snapping players onto the ground. */
  addAfterStepListener: (callback: CharacterStepCallback) => void,
  removeAfterStepListener: (callback: CharacterStepCallback) => void,
}

export const clientCharacterService: ClientCharacterService = {
  // ------------ before step ------------
  addBeforeStepListener: (callback: CharacterStepCallback) => {
    ourGlobals.__onBeforeCharacterStepCallbacks__.push(callback);
  },
  removeBeforeStepListener: (callback: CharacterStepCallback) => {
    removeFnFromArray(ourGlobals.__onBeforeCharacterStepCallbacks__, callback);
  },

  // ------------ after step ------------
  addAfterStepListener: (callback: CharacterStepCallback) => {
    ourGlobals.__onAfterCharacterStepCallbacks__.push(callback);
  },
  removeAfterStepListener: (callback: CharacterStepCallback) => {
    removeFnFromArray(ourGlobals.__onAfterCharacterStepCallbacks__, callback);
  },
}
