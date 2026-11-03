import assert from "node:assert/strict";
import test from "node:test";
import { getApiTypeDocsSections, getApiValueDocs, getObjectTypeDocs, getObjectTypeDocsPage, type ObjectTypeDoc, type ObjectTypeDocField } from "../packages/website/lib/objectTypeDocs.ts";

const findObjectTypeDoc = (objectTypeDocs: ObjectTypeDoc[], objectType: string): ObjectTypeDoc => {
  const objectTypeDoc = objectTypeDocs.find((candidateObjectTypeDoc) => candidateObjectTypeDoc.objectType === objectType);
  assert.notEqual(objectTypeDoc, undefined);
  if (objectTypeDoc === undefined) throw new Error(`Missing ${objectType} docs.`);
  return objectTypeDoc;
};

const findField = (objectTypeDoc: { objectType: string; fields: ObjectTypeDocField[] }, field: string): ObjectTypeDocField => {
  const objectTypeDocField = objectTypeDoc.fields.find((candidateObjectTypeDocField) => candidateObjectTypeDocField.field === field);
  assert.notEqual(objectTypeDocField, undefined);
  if (objectTypeDocField === undefined) throw new Error(`Missing ${objectTypeDoc.objectType}.${field} docs.`);
  return objectTypeDocField;
};

test("getObjectTypeDocs lists every ObjectInfo type in order", () => {
  const objectTypeDocs = getObjectTypeDocs();

  assert.deepEqual(objectTypeDocs.map((objectTypeDoc) => objectTypeDoc.objectType), [
    "ball",
    "brick",
    "clock",
    "cylinder",
    "group",
    "label",
    "labelImage",
    "material",
    "mesh",
    "pivot",
    "player",
    "propertyAnimation",
    "propertyAnimator",
    "propertyKeyframe",
    "rigidGroup",
    "script",
    "skAnimation",
    "skAnimator",
    "skAttachment",
    "skeleton",
    "skJoint",
    "skKeyframe",
    "skKeyframePose",
    "soundPlayer",
    "tween",
    "worldLighting",
  ]);
});

test("getObjectTypeDocs expands intersection fields", () => {
  const ballDoc = findObjectTypeDoc(getObjectTypeDocs(), "ball");
  const cylinderDoc = findObjectTypeDoc(getObjectTypeDocs(), "cylinder");

  assert.deepEqual(ballDoc.fields.map((field) => field.field), [
    "material",
    "opacity",
    "color",
    "glow",
    "isAlwaysOnTop",
    "radius",
    "location",
    "movement",
    "isWalkthrough",
    "isPushable",
    "density",
  ]);
  assert.deepEqual(cylinderDoc.fields.map((field) => field.field), [
    "material",
    "opacity",
    "color",
    "glow",
    "isAlwaysOnTop",
    "radius",
    "length",
    "location",
    "movement",
    "isWalkthrough",
    "isPushable",
    "density",
  ]);
});

test("getObjectTypeDocs keeps docstrings and object id reference markers", () => {
  const objectTypeDocs = getObjectTypeDocs();
  const skeletonDoc = findObjectTypeDoc(objectTypeDocs, "skeleton");
  const clockDoc = findObjectTypeDoc(objectTypeDocs, "clock");
  const groupDoc = findObjectTypeDoc(objectTypeDocs, "group");
  const skJointDoc = findObjectTypeDoc(objectTypeDocs, "skJoint");
  const skAnimationDoc = findObjectTypeDoc(objectTypeDocs, "skAnimation");
  const propertyAnimationDoc = findObjectTypeDoc(objectTypeDocs, "propertyAnimation");
  const propertyKeyframeDoc = findObjectTypeDoc(objectTypeDocs, "propertyKeyframe");
  const skKeyframeDoc = findObjectTypeDoc(objectTypeDocs, "skKeyframe");
  const skKeyframePoseDoc = findObjectTypeDoc(objectTypeDocs, "skKeyframePose");
  const rigidGroupDoc = findObjectTypeDoc(objectTypeDocs, "rigidGroup");
  const playerDoc = findObjectTypeDoc(objectTypeDocs, "player");
  const scriptDoc = findObjectTypeDoc(objectTypeDocs, "script");
  const ballDoc = findObjectTypeDoc(objectTypeDocs, "ball");
  const cylinderDoc = findObjectTypeDoc(objectTypeDocs, "cylinder");
  const skAnimatorDoc = findObjectTypeDoc(objectTypeDocs, "skAnimator");
  const propertyAnimatorDoc = findObjectTypeDoc(objectTypeDocs, "propertyAnimator");
  const meshDoc = findObjectTypeDoc(objectTypeDocs, "mesh");

  assert.deepEqual(skeletonDoc.fields, []);
  assert.equal("labels" in skeletonDoc, false);
  assert.equal("labels" in groupDoc, false);
  assert.equal("labels" in skJointDoc, false);
  assert.equal("labels" in skAnimationDoc, false);
  assert.equal("labels" in skKeyframeDoc, false);
  assert.equal(propertyKeyframeDoc.docstring, "A child of PropertyAnimation.");
  assert.equal(findField({ objectType: "propertyKeyframe", fields: propertyKeyframeDoc.fields }, "value").type, "number");
  assert.equal(findField({ objectType: "propertyAnimation", fields: propertyAnimationDoc.fields }, "duration").type, "number");
  assert.equal(skKeyframePoseDoc.docstring, "A child of SkKeyframe.\n\nThe pose of an individual joint.");
  assert.equal("labels" in skKeyframePoseDoc, false);
  assert.equal(findField(clockDoc, "clockState").docstring, "Whether it is paused or playing, and how far along it is.");
  assert.equal(scriptDoc.docstring.startsWith("Every Script is either a Server or Client script."), true);
  assert.equal(rigidGroupDoc.docstring.startsWith("A RigidGroup is a bunch of objects that move together."), true);
  assert.equal(findField(rigidGroupDoc, "followSkeletonId").docstring, "The objectId of the Skeleton that this rigid group will follow.");
  assert.equal(findField(playerDoc, "headObjectId").docstring, "The objectId of the Ball, Brick, Cylinder, or RigidGroup the player's camera will follow.");
  assert.equal(cylinderDoc.docstring, "A cylinder in the world. Its symmetry axis is local Z.");
  assert.equal(findField(playerDoc, "characterObjectId").docstring, "The objectId of the Ball, Brick, Cylinder, or RigidGroup this player controls.");
  assert.equal(findField(playerDoc, "headObjectId").isObjectIdReference, true);
  assert.equal(findField(ballDoc, "material").isObjectIdReference, true);
  assert.equal(findField(ballDoc, "material").docstring, "Visual Material texture settings.");
  assert.equal(findField(ballDoc, "radius").isObjectIdReference, false);
  assert.equal(findField(cylinderDoc, "material").isObjectIdReference, true);
  assert.equal(findField(cylinderDoc, "material").docstring, "Visual Material texture settings.");
  assert.equal(findField(cylinderDoc, "radius").isObjectIdReference, false);
  assert.equal(findField(cylinderDoc, "length").isObjectIdReference, false);
  assert.equal(findField(skAnimatorDoc, "skeletonId").docstring, "The Skeleton to animate.");
  assert.equal(findField(skAnimatorDoc, "priority").docstring, "0-10.");
  assert.equal(findField(skAnimatorDoc, "weight").docstring, "0-100.");
  assert.equal(skAnimatorDoc.docstring.startsWith("This animates a Skeleton."), true);
  assert.equal(propertyAnimatorDoc.docstring.startsWith("Use this to smoothly animate one property on any object."), true);
  assert.equal(findField(propertyAnimatorDoc, "targetObjectId").isObjectIdReference, true);
  assert.equal(findField(propertyAnimatorDoc, "propertyPath").docstring, 'Path to the continuous field to animate, for example ["color", "r"], ["opacity"], or ["location", "position", "x"].');
  assert.equal(findField(propertyAnimatorDoc, "clockId").isObjectIdReference, true);
  assert.equal(findField(propertyAnimatorDoc, "priority").docstring, "0-10.");
  assert.equal(findField(propertyAnimatorDoc, "weight").docstring, "0-100.");
  assert.equal(findField(meshDoc, "followSkeletonId").docstring, "The objectId of the Skeleton that this mesh will visually follow.");
  assert.equal(findField(meshDoc, "physicalObjectId").docstring, "The objectId of the Ball, Brick, Cylinder, or RigidGroup that the mesh will use as a collider and visually follow.");
  assert.equal(meshDoc.docstring, "A mesh in the world.\n\nTo animate this mesh visually, you must set followSkeletonId and use a SkAnimator to animate it.");
});

test("getObjectTypeDocs documents the world lighting sky gradient", () => {
  const worldLightingDoc = findObjectTypeDoc(getObjectTypeDocs(), "worldLighting");

  assert.equal(worldLightingDoc.docstring, "Controls the sun location and sky settings.");
  assert.equal(findField(worldLightingDoc, "skyTopColor").docstring, "The gradient sky color straight overhead.");
  assert.equal(findField(worldLightingDoc, "skyHorizonColor").docstring, "The gradient sky color at the horizon, which both halves of the gradient start from.");
  assert.equal(findField(worldLightingDoc, "skyBottomColor").docstring, "The gradient sky color straight down, below the horizon.");
  assert.equal(findField(worldLightingDoc, "skyboxTextureDataId").docstring, "An equirectangular skybox TextureData id. Transparent pixels reveal the gradient sky.");
  assert.equal(findField(worldLightingDoc, "skyboxTextureOpacity").docstring, "0 means the skybox texture is hidden. 1 means the skybox texture is fully visible.");
});

test("getObjectTypeDocsPage includes non-object reference types", () => {
  const { typeReferenceDocs } = getObjectTypeDocsPage();
  const vec3Doc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "Vec3");
  const ballMaterialSettingsDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "BallMaterialSettings");
  const cylinderMaterialSettingsDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "CylinderMaterialSettings");
  const ballInfoDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "BallInfo");
  const clockStateDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "ClockState");
  const weightTransitionDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "WeightTransition");
  const objectInfoDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "ObjectInfo");
  const objectLocationDoc = typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === "ObjectLocation");

  assert.notEqual(vec3Doc, undefined);
  assert.notEqual(ballMaterialSettingsDoc, undefined);
  assert.notEqual(cylinderMaterialSettingsDoc, undefined);
  assert.notEqual(clockStateDoc, undefined);
  assert.notEqual(weightTransitionDoc, undefined);
  assert.notEqual(objectInfoDoc, undefined);
  assert.notEqual(objectLocationDoc, undefined);
  assert.equal(ballInfoDoc, undefined);
  for (const removedTypeName of ["Clock" + "Location", "Clock" + "Reference", "Clock" + "Time", "Property" + "Animation" + "Value", "Script" + "Type", "Tween" + "Value", "Tween" + "Settings", "Weight" + "Transition" + "Settings"]) assert.equal(typeReferenceDocs.find((typeReferenceDoc) => typeReferenceDoc.typeName === removedTypeName), undefined);
  if (vec3Doc === undefined) return;
  if (ballMaterialSettingsDoc === undefined) return;
  if (cylinderMaterialSettingsDoc === undefined) return;
  if (clockStateDoc === undefined) return;
  if (weightTransitionDoc === undefined) return;
  if (objectInfoDoc === undefined) return;
  if (objectLocationDoc === undefined) return;

  assert.deepEqual(vec3Doc.fields.map((field) => field.field), ["x", "y", "z"]);
  assert.equal(objectInfoDoc.type.includes("BallInfo"), true);
  assert.equal(objectInfoDoc.type.includes("CylinderInfo"), true);
  assert.equal(objectInfoDoc.docstring.startsWith("This is a helper type. It contains all the fields of either a Ball"), true);
  assert.equal(objectInfoDoc.fields.length, 0);
  assert.equal(clockStateDoc.type.includes("localStartMs"), true);
  assert.equal(clockStateDoc.type.includes("timeLocation"), true);
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "startedAtMs").type, "number");
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "timeLocation").type, "ObjectLocation");
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "initValue").type, "number");
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "finalValue").type, "number");
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "durationMs").type, "number");
  assert.equal(findField({ objectType: "WeightTransition", fields: weightTransitionDoc.fields }, "easing").type, "InterpolationStyle");
  assert.equal(objectLocationDoc.type, '"Server" | "Client"');
  assert.equal(findField({ objectType: "BallMaterialSettings", fields: ballMaterialSettingsDoc.fields }, "materialInfoId").docstring, "The objectId of the Material to use.");
  assert.equal(findField({ objectType: "CylinderMaterialSettings", fields: cylinderMaterialSettingsDoc.fields }, "materialInfoId").docstring, "The objectId of the Material to use.");
});

test("getApiTypeDocsSections lists exported public entrypoint types", () => {
  const sectionOfName = Object.fromEntries(getApiTypeDocsSections().map((section) => [section.sectionName, section]));

  assert.deepEqual(sectionOfName["World"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["LiveObject"]);
  const liveObjectDoc = sectionOfName["World"].typeDocs[0];
  assert.equal(findField({ objectType: "LiveObject", fields: liveObjectDoc.fields }, "parent").type, "LiveObject | null");
  assert.ok(liveObjectDoc.fields.some((field) => field.field === "getChild"));
  assert.deepEqual(sectionOfName["Player Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["JsonValue", "PlayerService"]);
  assert.deepEqual(sectionOfName["Object Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["ObjectCreatedCallback", "ObjectDeletedCallback", "RaycastHit", "RaycastOptions", "ShapeCastOptions", "GroundSupportOptions", "ObjectService"]);
  assert.deepEqual(sectionOfName["Animation Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["AnimationService"]);
  assert.deepEqual(sectionOfName["Clock Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["ClockService"]);
  assert.deepEqual(sectionOfName["Collision Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["CollisionCallback", "CollisionService"]);
  assert.deepEqual(sectionOfName["GUI Types"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["GuiRenderable", "GuiRootRenderable"]);
  assert.deepEqual(sectionOfName["GUI Service"]?.typeDocs.map((typeDoc) => typeDoc.typeName), ["GuiService"]);

  const objectServiceDoc = sectionOfName["Object Service"]?.typeDocs.find((typeDoc) => typeDoc.typeName === "ObjectService");
  const guiRenderableDoc = sectionOfName["GUI Types"]?.typeDocs.find((typeDoc) => typeDoc.typeName === "GuiRenderable");
  assert.notEqual(objectServiceDoc, undefined);
  assert.notEqual(guiRenderableDoc, undefined);
  if (objectServiceDoc === undefined) return;
  if (guiRenderableDoc === undefined) return;
  assert.equal(guiRenderableDoc.docstring.startsWith("This is the equivalent to a React Node."), true);
});

test("getApiValueDocs lists selected exported public values", () => {
  const valueDocs = getApiValueDocs(["stopScript", "tick", "useState", "createState"]);

  assert.deepEqual(valueDocs.map((valueDoc) => valueDoc.typeName), ["stopScript", "tick", "useState", "createState"]);
  assert.equal(valueDocs.find((valueDoc) => valueDoc.typeName === "stopScript")?.type, "() => Promise<never>");
  assert.equal(valueDocs.find((valueDoc) => valueDoc.typeName === "stopScript")?.docstring, "Use `await stopScript()` to stop all further execution of a script.");
  assert.equal(valueDocs.find((valueDoc) => valueDoc.typeName === "tick")?.type, "(__script_object_id__?: unknown) => Promise<void>");
  assert.equal(valueDocs.find((valueDoc) => valueDoc.typeName === "useState")?.type.includes("<T>"), true);
  assert.equal(valueDocs.find((valueDoc) => valueDoc.typeName === "createState")?.type.includes("get: () =>"), true);
});
