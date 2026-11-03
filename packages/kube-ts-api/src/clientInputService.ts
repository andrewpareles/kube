import { denoOps, ourGlobals } from "./object_types/__globalOps__";
import { removeFnFromArray } from "./lib/removeFnFromArray";
import type { DataId, Vec3 } from "./object_types/shared";

/** Physical (layout-independent) key names, so WASD stays WASD on any keyboard layout. */
export type KeyName =
  | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M"
  | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z"
  | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
  | "Space" | "ShiftLeft" | "ShiftRight"
  | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

export type MouseButtonName = "left" | "right";

export type KeyCallback = (key: KeyName) => void;
export type MouseButtonCallback = (button: MouseButtonName) => void;

export type MouseRay = {
  /** */
  origin: Vec3,
  /** Unit vector. */
  direction: Vec3,
};

/** The cursors built into the game: an arrow, a pointing hand, and four short lines around the point that clicks. */
export type BuiltinMouseCursor = "default" | "pointer" | "crosshair";

/** A picture to use as the mouse cursor. */
export type MouseCursorImage = {
  /** */
  textureDataId: DataId,
  /** Size on screen in points, so it looks the same on any display. At most 128. */
  width: number,
  height: number,
  /** The point that clicks, as a fraction of the picture. 0, 0 is the top left (an arrow's tip). 0.5, 0.5 is the center (a crosshair). */
  hotspotX: number,
  hotspotY: number,
};

export type MouseCursor = BuiltinMouseCursor | MouseCursorImage;

/** This service exposes the local player's raw input to Client Scripts. Only callable by a Client Script. 
 *
 * - keyDown is fired once per key press. To detect if a key is held down (or currently pressed), use `isKeyDown`.
 * - Clicks that land on script GUI (like `<button>`) are consumed by the GUI and do not fire mouse listeners.
 * - When the game window loses focus, key up / mouse up fires for everything currently held.
 * - To react to input on the server, forward what you need through a 'use server' function.
 *
 * Example usage: `game.clientInputService.addKeyDownListener((key) => { if (key === "E") interact(); })`.
 * */
export type ClientInputService = {
  addKeyDownListener: (callback: KeyCallback) => void,
  removeKeyDownListener: (callback: KeyCallback) => void,

  addKeyUpListener: (callback: KeyCallback) => void,
  removeKeyUpListener: (callback: KeyCallback) => void,

  isKeyDown: (key: KeyName) => boolean,

  // TODO: expose the camera zoom (as a readable constant) to scripts in the future, instead of raw scroll wheel events.

  /** Does not fire for presses on a script GUI's buttons, or on GUI elements with onClick, onRightClick, onMouseDown or onMouseUp. */
  addMouseDownListener: (callback: MouseButtonCallback) => void,
  removeMouseDownListener: (callback: MouseButtonCallback) => void,

  addMouseUpListener: (callback: MouseButtonCallback) => void,
  removeMouseUpListener: (callback: MouseButtonCallback) => void,

  isMouseButtonDown: (button: MouseButtonName) => boolean,

  /** The unit vector the local player's camera looks along.
   * You can pair this with the `castRay()` function. */
  getCameraDirection: () => Vec3,
  /**  Lock the player's camera to face the given direction. */
  lockCameraDirection: (direction: Vec3) => void,
  /**  */
  unlockCameraDirection: () => void,
  /** The player can only zoom between these distances from the head. Defaults are 0.2 and 200. Set both equal to lock the zoom. */
  setZoomRange: (minZoom: number, maxZoom: number) => void,
  /** The camera's distance from the head right now. */
  getZoom: () => number,
  /** The zoom the camera wants to go to. Things in the way move the camera closer, use `getZoom()` to get the real zoom. */
  getDesiredZoom: () => number,
  /** Set the player's field of view in degrees, from 1 to 179. This is the vertical angle the camera sees. Defaults to 70. Pass null to go back to the default. */
  setFOV: (degrees: number | null) => void,
  /** Sets the camera to be offset from the head it tracks along its own x (right), y (up) and z (forward) axes, like `{ x: 1.5, y: 0, z: 0 }` for a view over the right shoulder. Defaults to 0, 0, 0. */
  setCameraOffset: (offset: Vec3) => void,

  /** Moves the player's mouse to the middle of the screen and holds it there, like a first-person game.
   * Moving the mouse then turns the camera (unless its direction is locked), and clicks land in the middle, so `getMouseRay()` points through it.
   * Unlock it when the player needs to click a GUI. */
  lockMouse: () => void,
  /** Lets the mouse move again, from the middle of the screen. */
  unlockMouse: () => void,

  /** Changes what the local player's mouse cursor looks like. It starts as "default". */
  setMouseCursor: (mouseCursor: MouseCursor) => void,

  /** The ray the local player's mouse cursor points along, or null if the mouse is off the screen.
    * You can pair this with the `castRay()` function. */
  getMouseRay: () => MouseRay | null,
}

export const clientInputService: ClientInputService = {
  // ------------ key down ------------
  addKeyDownListener: (callback: KeyCallback) => {
    ourGlobals.__onKeyDownCallbacks__.push(callback);
  },
  removeKeyDownListener: (callback: KeyCallback) => {
    removeFnFromArray(ourGlobals.__onKeyDownCallbacks__, callback);
  },

  // ------------ key up ------------
  addKeyUpListener: (callback: KeyCallback) => {
    ourGlobals.__onKeyUpCallbacks__.push(callback);
  },
  removeKeyUpListener: (callback: KeyCallback) => {
    removeFnFromArray(ourGlobals.__onKeyUpCallbacks__, callback);
  },

  // ------------ key state ------------
  isKeyDown: (key: KeyName) => {
    return ourGlobals.__pressedKeyNames__.has(key);
  },

  // ------------ mouse down ------------
  addMouseDownListener: (callback: MouseButtonCallback) => {
    ourGlobals.__onMouseDownCallbacks__.push(callback);
  },
  removeMouseDownListener: (callback: MouseButtonCallback) => {
    removeFnFromArray(ourGlobals.__onMouseDownCallbacks__, callback);
  },

  // ------------ mouse up ------------
  addMouseUpListener: (callback: MouseButtonCallback) => {
    ourGlobals.__onMouseUpCallbacks__.push(callback);
  },
  removeMouseUpListener: (callback: MouseButtonCallback) => {
    removeFnFromArray(ourGlobals.__onMouseUpCallbacks__, callback);
  },

  // ------------ mouse state ------------
  isMouseButtonDown: (button: MouseButtonName) => {
    return ourGlobals.__pressedMouseButtonNames__.has(button);
  },

  // ------------ camera ------------
  getCameraDirection: () => {
    return denoOps.__getCameraDirection__();
  },
  lockCameraDirection: (direction: Vec3) => {
    denoOps.__lockCameraDirection__(JSON.stringify(direction));
  },
  unlockCameraDirection: () => {
    denoOps.__unlockCameraDirection__();
  },
  setZoomRange: (minZoom: number, maxZoom: number) => {
    denoOps.__setZoomRange__(minZoom, maxZoom);
  },
  getZoom: () => {
    return denoOps.__getZoom__();
  },
  getDesiredZoom: () => {
    return denoOps.__getDesiredZoom__();
  },
  setFOV: (degrees: number | null) => {
    denoOps.__setFOV__(degrees);
  },
  setCameraOffset: (offset: Vec3) => {
    denoOps.__setCameraOffset__(JSON.stringify(offset));
  },

  // ------------ mouse lock ------------
  lockMouse: () => {
    denoOps.__setIsMouseLocked__(true);
  },
  unlockMouse: () => {
    denoOps.__setIsMouseLocked__(false);
  },

  // ------------ mouse cursor ------------
  setMouseCursor: (mouseCursor: MouseCursor) => {
    denoOps.__setMouseCursor__(JSON.stringify(mouseCursor));
  },

  // ------------ mouse ray ------------
  getMouseRay: () => {
    return denoOps.__getMouseRay__();
  },
}
