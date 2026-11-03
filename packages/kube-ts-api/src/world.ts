import { makeObservable } from "./lib/makeObservable";
import { denoOps } from "./object_types/__globalOps__";
import { ObjectInfo, RustObjectSnapshot } from "./object_types/types";
import type { ObjectId } from "./object_types/shared";
import { cloneJsValue } from "./lib/cloneJsValue";

// --------------------- Helpers ---------------------

// Reads of an object share one copy until the world might have changed, which Rust counts.
let rustObjectOfIdWorldChangeCount = -1;
let rustObjectOfId: { [objectId: number]: RustObjectSnapshot | null } = {};
const readRustObject = (objectId: ObjectId): RustObjectSnapshot | null => {
  const worldChangeCount = denoOps.__getWorldChangeCount__();
  if (worldChangeCount !== rustObjectOfIdWorldChangeCount) {
    rustObjectOfIdWorldChangeCount = worldChangeCount;
    rustObjectOfId = {};
  }
  if (!(objectId in rustObjectOfId)) rustObjectOfId[objectId] = denoOps.__getRustObject__(objectId);
  return rustObjectOfId[objectId];
};

// Read current fields from Rust and send field assignments back to Rust.
const listenForDeepChanges = (o: { id: ObjectId }): LiveObject => {
  // Nested references keep their object ID and path, so they also see changes made by physics or another handle.
  return makeObservable(o, () => readRustObject(o.id), (newValue, path) => {
    // if they ran delete x[y]
    // if they ran x[y] = z
    // we are stripping the object down for efficiency, and to remove any functions on the JSON so serde doesn't give an error
    // we clone so modifying this object doesnt modify the original object
    const value = cloneJsValue(newValue === undefined ? null : newValue);
    denoOps.__setRustObjectField__(o.id, JSON.stringify(path), JSON.stringify(value));
  }, getRustObject) as LiveObject;
};

/** Scripts usually deal with this object type. 
 * 
 * It contains all the fields of either a 
 * Ball, Brick, Clock, Cylinder, Label, LocalGroup, SkJoint,
 * SkAnimation, SkAnimator, SkAttachment,
 * SkKeyframe, SkKeyframePose, Material, Mesh, Pivot, Player, PropertyAnimation, PropertyAnimator,
 * PropertyKeyframe, RigidGroup, Script, Skeleton, SoundPlayer, Tween, or WorldLighting.
 * 
 * This object is a proxy, meaning every time you read a value from it, you get the current value in the world.
 * You can also set fields on it, and they will affect the real object in the world. 
 * If the object is deleted, the `id` remains readable, but other fields will throw an error. Use `getObjectById()`, or a try/catch, if you think the object might be deleted.
 * 
 * And it contains the following additional fields: */
export type LiveObject = {
  readonly id: ObjectId;
  name: string;
  parent: LiveObject | null;
  // getRustObject helpers
  getChild: (name: string) => LiveObject | null;
  getChildren: (name?: string) => LiveObject[];
} & ObjectInfo;

const getChildrenFromId = (objectId: number): LiveObject[] => {
  const childrenIds = denoOps.__getRustChildrenIds__(objectId);
  const children: LiveObject[] = [];
  for (const id of childrenIds) {
    let child = getRustObject(id);
    if (child === null) continue;
    children.push(child);
  }
  return children;
};

export const getRustObject = (objectId: number): LiveObject | null => {
  if (readRustObject(objectId) === null) return null;
  // Every field is read through readRustObject, so this object only holds the id and the helpers below.
  const o = { id: objectId };
  Object.defineProperty(o, "id", { writable: false, configurable: false });

  // Add helpers that are safe to attach directly to the Rust-backed object.
  // .getChild(name)
  Object.defineProperty(o, "getChild", {
    value: (name: string) => {
      return getChildrenFromId(objectId).find((child) => child.name === name) ?? null;
    },
    enumerable: false,
  });

  // .getChildren()
  Object.defineProperty(o, "getChildren", {
    value: (name?: string) => {
      const children = getChildrenFromId(objectId);
      if (name === undefined) return children;
      return children.filter((child) => child.name === name);
    },
    enumerable: false,
  });

  // The shared proxy handles field reads and writes, including parent ID conversion.
  return listenForDeepChanges(o);
};
const rootObjId = denoOps.__getRustRootObjectId__();
export const world = getRustObject(rootObjId);
