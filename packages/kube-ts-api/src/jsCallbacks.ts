import { ObjectId } from "./object_types/shared";
import { denoOps, ourGlobals } from "./object_types/__globalOps__";
import { getRustObject } from "./world";
import type { GameChatMessage } from "./gameChatService";
import type { KeyName, MouseButtonName } from "./clientInputService";
import type { ObjectInfo } from "./object_types/types";
import type { CharacterStepCallback } from "./clientCharacterService";
import { removeFnFromArray } from "./lib/removeFnFromArray";

export type JsCallbackInputs =
  | { ObjectCreated: { object_id: ObjectId; }; }
  | { ObjectDeleted: { object_id: ObjectId; object_type: ObjectInfo["type"]; }; } // type is needed so we can filter
  | { CollisionStarted: { object_id_1: ObjectId; object_id_2: ObjectId; }; }
  | { CollisionStopped: { object_id_1: ObjectId; object_id_2: ObjectId; }; }
  | { KeyDown: { key: KeyName; }; }
  | { KeyUp: { key: KeyName; }; }
  | { MouseDown: { button: MouseButtonName; }; }
  | { MouseUp: { button: MouseButtonName; }; }
  | { GameChatMessageSent: GameChatMessage; }
  | { BeforeCharacterStep: { character_object_id: ObjectId; dt_secs: number; }; }
  | { AfterCharacterStep: { character_object_id: ObjectId; dt_secs: number; }; };


// A callback written in a script that has since been deleted is forgotten instead of called. Looping over a copy lets a callback remove itself.
const callLiveCallbacks = <Callback extends (...args: never[]) => unknown>(callbacks: Callback[], call: (callback: Callback) => void) => {
  for (const callback of [...callbacks]) {
    if (!denoOps.__isFnFromLiveScript__(callback)) {
      removeFnFromArray(callbacks, callback);
      continue;
    }
    call(callback);
  }
}

// Each callback's changes are applied before the next callback runs, so later callbacks see them.
// Step callbacks run on every physics step, so one that throws is reported without skipping the rest. Looping over a copy lets a callback remove itself.
const runCharacterStepCallbacks = (callbacks: CharacterStepCallback[], characterObjectId: ObjectId, dtSecs: number) => {
  callLiveCallbacks(callbacks, (callback) => {
    try {
      const characterChanges = callback(characterObjectId, dtSecs);
      if (characterChanges === undefined) return;
      if (characterChanges instanceof Promise) throw new Error("A character step callback must return its changes right away, but one of yours was async.");
      denoOps.__applyCharacterChanges__(JSON.stringify(characterChanges));
    } catch (error) {
      console.error("[Unhandled JS error]", error);
    }
  });
}

const runAllJsCallbacks = (jsCallbackInputsList: JsCallbackInputs[]) => {
  for (const jsCallbackInputs of jsCallbackInputsList) {
    // type-filtered callbacks run before unfiltered ("undefined" bucket) callbacks
    if ("ObjectCreated" in jsCallbackInputs) {
      const object = getRustObject(jsCallbackInputs.ObjectCreated.object_id);
      // the object can legitimately be gone already if it was created and deleted before this batch ran
      if (object !== null) {
        callLiveCallbacks(ourGlobals.__onObjectCreatedCallbacks__[object.type] ?? [], (callback) => callback(object));
        callLiveCallbacks(ourGlobals.__onObjectCreatedCallbacks__[`${undefined}`] ?? [], (callback) => callback(object));
      }
    }

    if ("ObjectDeleted" in jsCallbackInputs) {
      const deletedObjectId = jsCallbackInputs.ObjectDeleted.object_id;
      const deletedObjectType = jsCallbackInputs.ObjectDeleted.object_type;
      callLiveCallbacks(ourGlobals.__onObjectDeletedCallbacks__[deletedObjectType] ?? [], (callback) => callback(deletedObjectId, deletedObjectType));
      callLiveCallbacks(ourGlobals.__onObjectDeletedCallbacks__[`${undefined}`] ?? [], (callback) => callback(deletedObjectId, deletedObjectType));
    }

    if ("CollisionStarted" in jsCallbackInputs) {
      if (ourGlobals.__onCollisionCallbacks__.length === 0) continue;
      const object1 = getRustObject(jsCallbackInputs.CollisionStarted.object_id_1);
      const object2 = getRustObject(jsCallbackInputs.CollisionStarted.object_id_2);
      if (object1 === null || object2 === null) {
        console.error(
          "Skipping collision start callback because a collision object was missing. This is unexpected and should never happen.",
          jsCallbackInputs.CollisionStarted.object_id_1,
          jsCallbackInputs.CollisionStarted.object_id_2,
        );
        continue;
      }
      callLiveCallbacks(ourGlobals.__onCollisionCallbacks__, (callback) => callback(object1, object2));
    }

    // pressed sets are updated before the callbacks run, so isKeyDown/isMouseButtonDown inside a listener reflects this event
    if ("KeyDown" in jsCallbackInputs) {
      const key = jsCallbackInputs.KeyDown.key;
      ourGlobals.__pressedKeyNames__.add(key);
      callLiveCallbacks(ourGlobals.__onKeyDownCallbacks__, (callback) => callback(key));
    }

    if ("KeyUp" in jsCallbackInputs) {
      const key = jsCallbackInputs.KeyUp.key;
      ourGlobals.__pressedKeyNames__.delete(key);
      callLiveCallbacks(ourGlobals.__onKeyUpCallbacks__, (callback) => callback(key));
    }

    if ("MouseDown" in jsCallbackInputs) {
      const button = jsCallbackInputs.MouseDown.button;
      ourGlobals.__pressedMouseButtonNames__.add(button);
      callLiveCallbacks(ourGlobals.__onMouseDownCallbacks__, (callback) => callback(button));
    }

    if ("MouseUp" in jsCallbackInputs) {
      const button = jsCallbackInputs.MouseUp.button;
      ourGlobals.__pressedMouseButtonNames__.delete(button);
      callLiveCallbacks(ourGlobals.__onMouseUpCallbacks__, (callback) => callback(button));
    }

    // The server has already accepted and censored this, so every listener reads what players read.
    if ("GameChatMessageSent" in jsCallbackInputs) {
      callLiveCallbacks(ourGlobals.__onGameChatMessageCallbacks__, (callback) => callback(jsCallbackInputs.GameChatMessageSent));
    }

    if ("CollisionStopped" in jsCallbackInputs) {
      if (ourGlobals.__onCollisionStopCallbacks__.length === 0) continue;
      const object1 = getRustObject(jsCallbackInputs.CollisionStopped.object_id_1);
      const object2 = getRustObject(jsCallbackInputs.CollisionStopped.object_id_2);
      if (object1 === null || object2 === null) {
        console.error(
          "Skipping collision stop callback because a collision object was missing. This is unexpected and should never happen.",
          jsCallbackInputs.CollisionStopped.object_id_1,
          jsCallbackInputs.CollisionStopped.object_id_2,
        );
        continue;
      }
      callLiveCallbacks(ourGlobals.__onCollisionStopCallbacks__, (callback) => callback(object1, object2));
    }

    if ("BeforeCharacterStep" in jsCallbackInputs) {
      const { character_object_id, dt_secs } = jsCallbackInputs.BeforeCharacterStep;
      runCharacterStepCallbacks(ourGlobals.__onBeforeCharacterStepCallbacks__, character_object_id, dt_secs);
    }

    if ("AfterCharacterStep" in jsCallbackInputs) {
      const { character_object_id, dt_secs } = jsCallbackInputs.AfterCharacterStep;
      runCharacterStepCallbacks(ourGlobals.__onAfterCharacterStepCallbacks__, character_object_id, dt_secs);
    }
  }
}

denoOps.__registerJsCallbackFn__(runAllJsCallbacks)

// A timer written in a script that has since been deleted stops instead of firing.
const denoSetTimeout = globalThis.setTimeout;
const denoSetInterval = globalThis.setInterval;
Object.assign(globalThis, {
  setTimeout: (callback: (...callbackArgs: unknown[]) => void, ms: number | undefined, ...callbackArgs: unknown[]) => {
    return denoSetTimeout(() => {
      if (denoOps.__isFnFromLiveScript__(callback)) callback(...callbackArgs);
    }, ms);
  },
  setInterval: (callback: (...callbackArgs: unknown[]) => void, ms: number | undefined, ...callbackArgs: unknown[]) => {
    const intervalId = denoSetInterval(() => {
      if (denoOps.__isFnFromLiveScript__(callback)) callback(...callbackArgs);
      else clearInterval(intervalId);
    }, ms);
    return intervalId;
  },
});
