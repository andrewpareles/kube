// This file should pretty much mirror the rust file with the same name, except for places I indicated.

export type Vec3 = { x: number; y: number; z: number };
// export type Quat = { x: number; y: number; z: number; w: number };

// -- All object properties --

/** Every object in the world gets a unique id that you can use to reference it. It's just a number. These object ids may change between compile time and runtime. We automatically convert most object ids, but you should not hard-code object ids into scripts as we can't parse those until runtime. */
export type ObjectId = number;

/** "Data" generally means large assets that are uploaded and given an id, like mesh data and texture data. Objects reference them using their data id. */
export type DataId = number;

/** The user-id of the user's account. For example, kube.new/profile/[user-id]. */
export type UserId = number;

// -- 3D object properties --
/** This is called "location", but really means both position and rotation. In other words, the full information needed to place the object in 3D at snapshot in time. Can be global or local depending on context. */
export type Location3D = {
  /** Distance along x, y, z. */
  position: Vec3;
  // DIFFERENT: quaternion -> rotation, radians -> degrees
  /** The object's x, y, z rotation in degrees with respect to the world, applied in order of y, x, z. */
  rotation: Vec3;
};

/** Speed information of an object. The derivative of Location3D. */
export type Movement3D = {
  /** Distance per second along x, y, z. */
  velocity: Vec3;
  // DIFFERENT: radians per second -> degrees per second
  /** Degrees per second around x, y, z. */
  angularVelocity: Vec3;
};

/** A helper type. */
export type PhysicsToggles3D = {
  /** Whether this object can pass through other objects. */
  isWalkthrough: boolean;
  /** Whether physics can move this object: gravity makes it fall and other objects push it. When false, it is totally unmovable. */
  isPushable: boolean;
  density: number;
};

/** RGB color. */
export type Color3 = {
  /** Red on a scale of 0-1. */
  r: number;
  /** Green on a scale of 0-1. */
  g: number;
  /** Blue on a scale of 0-1. */
  b: number;
};

/** A helper type. */
export type VisualSettings = {
  /** 0-1. */
  opacity: number;
  color: Color3;
  /** 0-1. How much it shines in its own color whatever the light, and glows onto its surroundings, like neon. 0 is a normal object. */
  glow: number;
  /** Whether to render in front of everything even if visually blocked. This should almost never be used. */
  isAlwaysOnTop: boolean;
};

/** A helper type. */
export type PhysicsInfo3D = {
  location: Location3D;
  movement: Movement3D;
};

/** A helper type. */
export type Orientation3D = {
  // DIFFERENT: radians -> degrees, and phi is negated so positive means up in JS.
  /** Degrees around the vertical direction. Starts at Z and goes clockwise around Y. This is what changes when you look left or right.
   */
  theta: number;
  /** Degrees with respect to the horizon (XZ plane). Upwards is positive. This is what changes when you look up or down.
   */
  phi: number;
};

/** A helper type. */
export type CameraLocation = {
  position: Vec3;
  orientation: Orientation3D;
};
