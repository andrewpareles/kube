import type { Vec3 } from "./object_types/shared";

/** Common vector math functions for `Vec3`s. Every function returns a new vector.
 *
 * Example usage: `const velocity = vec3.scale(vec3.normalize(direction), speed)`.
 * */
export type Vec3Math = {
  add: (a: Vec3, b: Vec3) => Vec3,
  /** a minus b. */
  sub: (a: Vec3, b: Vec3) => Vec3,
  scale: (v: Vec3, multiplier: number) => Vec3,
  dot: (a: Vec3, b: Vec3) => number,
  cross: (a: Vec3, b: Vec3) => Vec3,
  length: (v: Vec3) => number,
  /** Length 1, or the zero vector if v has no length. */
  normalize: (v: Vec3) => Vec3,
  /**  */
  clampLength: (v: Vec3, maxLength: number) => Vec3,
  /**  */
  projectOntoPlane: (v: Vec3, planeNormal: Vec3) => Vec3,
}

export const vec3: Vec3Math = {
  add: (a: Vec3, b: Vec3) => {
    return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z };
  },
  sub: (a: Vec3, b: Vec3) => {
    return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z };
  },
  scale: (v: Vec3, multiplier: number) => {
    return { x: v.x * multiplier, y: v.y * multiplier, z: v.z * multiplier };
  },
  dot: (a: Vec3, b: Vec3) => {
    return a.x * b.x + a.y * b.y + a.z * b.z;
  },
  cross: (a: Vec3, b: Vec3) => {
    return { x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x };
  },
  length: (v: Vec3) => {
    return Math.hypot(v.x, v.y, v.z);
  },
  normalize: (v: Vec3) => {
    const length = vec3.length(v);
    if (length === 0) return { x: 0, y: 0, z: 0 };
    return vec3.scale(v, 1 / length);
  },
  clampLength: (v: Vec3, maxLength: number) => {
    const length = vec3.length(v);
    if (length <= maxLength) return { x: v.x, y: v.y, z: v.z };
    return vec3.scale(v, maxLength / length);
  },
  projectOntoPlane: (v: Vec3, planeNormal: Vec3) => {
    return vec3.sub(v, vec3.scale(planeNormal, vec3.dot(v, planeNormal)));
  },
}
