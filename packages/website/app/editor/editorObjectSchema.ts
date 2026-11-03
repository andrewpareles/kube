import { z } from 'zod'

// These schemas describe Rust's StoredWorld JSON, not the flattened, degree-based script API.
// The same definitions validate edits and describe the property controls.
export type EditorValue = z.infer<ReturnType<typeof z.json>>
export type EditorFieldSchema = z.core.JSONSchema.JSONSchema
// Rust writes each f32 in its shortest form, and f32::MAX prints as 3.4028235e38, just above z.float32()'s bound.
const number = z.number().min(-3.4028235e38).max(3.4028235e38)
const positive = number.nonnegative()
const unit = number.min(0).max(1)
const objectId = z.int().nonnegative()
const dataId = z.int()
const vector = z.tuple([number, number, number]).meta({ control: 'vector' })
const rotation = z.tuple([number, number, number, number]).meta({ control: 'rotation' })
const color = z.object({ r: unit, g: unit, b: unit }).meta({ control: 'color', default: { r: 0, g: 0, b: 0 } })
const location = z.object({ position: vector, quaternion: rotation })
const orientation = z.object({ theta: number.meta({ control: 'degrees' }), phi: number.meta({ control: 'negativeDegrees' }) })
const objectLocation = z.enum(['Client', 'Server'])
const interpolation = z.enum(['Step', 'Linear', 'EaseInOut', 'EaseIn', 'EaseOut'])
const reference = (objectKind: string) => objectId.meta({ control: 'reference', objectKind })
const visual = z.object({ opacity: unit, color, glow: unit, isAlwaysOnTop: z.boolean() })
const weightTransition = z.object({ startedAtMs: number, timeLocation: objectLocation, initValue: number, finalValue: number, durationMs: positive, easing: interpolation })
const animator = { clockId: reference('Clock').nullable(), priority: z.int().min(0).max(10), weight: number.min(0).max(100), weightTransition: weightTransition.nullable() }
// Match the desktop defaults when enabling material tiling, alpha masking, and label wrapping.
const primitiveMaterial = z.object({ materialInfoId: reference('Material'), tileSizeU: number.meta({ default: 1 }), tileSizeV: number.meta({ default: 1 }), tileOffsetU: number, tileOffsetV: number })
const primitiveInfo = z.object({ material: primitiveMaterial.nullable(), visualSettings: visual })
const animation = z.object({ duration: positive, interpolationStyle: interpolation })
const clockState = z.union([z.object({ Paused: z.object({ animationTimeSecs: number }) }), z.object({ Playing: z.object({ localStartMs: number, timeLocation: objectLocation }) })])

// Keep every object variant and field aligned with crates/game_types/src/object_types.
export const objectInfoSchemaOfKind: { [kind: string]: z.ZodObject } = {
    Ball: primitiveInfo,
    Brick: primitiveInfo,
    Cylinder: primitiveInfo,
    LocalGroup: z.object({}),
    Skeleton: z.object({}),
    RigidGroup: z.object({ followSkeletonId: reference('Skeleton').nullable() }),
    Script: z.object({ objectLocation }),
    Clock: z.object({ clockState, shouldLoop: z.boolean(), speed: number }),
    Material: z.object({ albedoTextureDataId: dataId.nullable(), metallicTextureDataId: dataId.nullable(), roughnessTextureDataId: dataId.nullable(), normalTextureDataId: dataId.nullable(), alphaMode: z.union([z.literal('Opaque'), z.object({ Mask: z.object({ cutoff: unit.meta({ default: 0.5 }) }) }), z.literal('Blend')]), isDoubleSided: z.boolean() }),
    Mesh: z.object({ meshDataId: dataId, scale: vector, material: z.object({ materialInfoId: reference('Material') }).nullable(), followSkeletonId: reference('Skeleton').nullable(), visualSettings: visual, physicalObjectId: reference('physical').nullable() }),
    Label: z.object({ text: z.string(), lineHeight: positive, maxWidth: positive.meta({ default: 4 }).nullable(), horizontalAlign: z.enum(['Left', 'Center', 'Right']), backgroundColor: color.nullable(), backgroundOpacity: unit, offsetFromObject: vector, rotationFromObject: rotation, physicalObjectId: reference('physical').nullable(), billboardMode: z.enum(['None', 'Full']), isConstantSize: z.boolean(), visualSettings: visual }),
    LabelImage: z.object({ textureDataId: dataId, width: positive, height: positive, offsetFromObject: vector, rotationFromObject: rotation, physicalObjectId: reference('physical').nullable(), billboardMode: z.enum(['None', 'Full']), isConstantSize: z.boolean(), visualSettings: visual }),
    Player: z.object({ username: z.string(), userid: z.int().nullable(), preferredColor: color.nullable(), equippedItemId: z.int().nullable(), headObjectId: reference('').nullable(), characterObjectId: reference('physical').nullable() }),
    SoundPlayer: z.object({ soundDataId: dataId, volume: positive, clockId: reference('Clock').nullable() }),
    SkJoint: z.object({ restPose: location }),
    SkAnimation: animation,
    PropertyAnimation: animation,
    SkKeyframe: z.object({ time: number }),
    PropertyKeyframe: z.object({ time: number, value: number }),
    SkKeyframePose: z.object({ jointName: z.string(), localPose: location }),
    SkAttachment: z.object({ objectId: reference('').nullable(), jointName: z.string(), offsetFromJoint: location }),
    SkAnimator: z.object({ skeletonId: reference('Skeleton').nullable(), skAnimationId: reference('SkAnimation').nullable(), ...animator }),
    PropertyAnimator: z.object({ targetObjectId: reference('').nullable(), propertyPath: z.array(z.string()), propertyAnimationId: reference('PropertyAnimation').nullable(), ...animator }),
    Tween: z.object({ targetObjectId: reference('').nullable(), propertyPath: z.array(z.string()), fromValue: number, toValue: number, duration: positive, interpolationStyle: interpolation, clockState }),
    Pivot: z.object({ physicalObjectId: reference('physical').nullable(), offsetFromObject: vector }),
    WorldLighting: z.object({ sunOrientation: orientation, sunColor: color, sunIntensity: positive, sunShadowSoftness: positive, ambientTopColor: color, ambientBottomColor: color, ambientIntensity: positive, ambientOcclusionIntensity: unit, ambientOcclusionRadius: positive, ambientOcclusionSunlightStrength: unit, skyTopColor: color, skyHorizonColor: color, skyBottomColor: color, skyboxTextureDataId: dataId.nullable(), skyboxTextureOpacity: unit, skyReflectionStrength: unit, vibrance: unit }),
}
export const physicsInfo3DSchema = z.object({ location, movement: z.object({ velocity: vector, angularVelocity: vector.meta({ control: 'angularVelocity' }) }) })
export const physicsToggles3DSchema = z.object({ isWalkthrough: z.boolean(), isPushable: z.boolean(), density: positive })
export const primitiveSchemaOfKind: { [kind: string]: z.ZodObject } = {
    Ball: z.object({ radius: positive }),
    Brick: z.object({ size: z.tuple([positive, positive, positive]).meta({ control: 'vector' }) }),
    Cylinder: z.object({ radius: positive, length: positive }),
}
const liveObjectSchema = z.object({ id: objectId, name: z.string(), info: z.record(z.string(), z.json()) })
export const storedWorldSchema = z.object({
    worldSchemaVersion: z.int(),
    fallback_camera_location: z.object({ position: vector, orientation }),
    objectOfId: z.record(z.string(), liveObjectSchema),
    rootObjectId: objectId,
    childrenOfId: z.record(z.string(), z.array(objectId)),
    scriptRawCodeOfId: z.record(z.string(), z.string()),
    physicsInfo3DOfId: z.record(z.string(), physicsInfo3DSchema),
    physicsToggles3DOfId: z.record(z.string(), physicsToggles3DSchema),
    primitiveSettingsOfId: z.record(z.string(), z.record(z.string(), z.json())),
})
export type StoredWorld = z.infer<typeof storedWorldSchema>
export type LiveObject = z.infer<typeof liveObjectSchema>
export const getObjectKind = (object: LiveObject) => Object.keys(object.info)[0]
export const getFieldLabel = (name: string) => name.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/^is /, '').replace(/^./, character => character.toUpperCase())
export const fieldSchemaOfKind = Object.fromEntries(Object.entries(objectInfoSchemaOfKind).map(([kind, schema]) => [kind, z.toJSONSchema(schema)]))

// Optional fields start with valid values; required object links need an existing target.
export const getInitialFieldValue = (schema: EditorFieldSchema, world: StoredWorld): EditorValue | undefined => {
    if (schema.default !== undefined) return z.json().parse(schema.default)
    if (schema.control === 'reference') return Object.values(world.objectOfId).find(object => isReferenceTarget(schema, world, object))?.id
    if (schema.anyOf) return getInitialFieldValue(schema.anyOf[0], world)
    if (schema.const !== undefined) return schema.const
    if (schema.enum) return schema.enum[0]
    if (schema.type === 'null') return null
    if (schema.type === 'boolean') return false
    if (schema.type === 'string') return ''
    if (schema.type === 'number' || schema.type === 'integer') return Math.max(0, schema.minimum ?? 0)
    if (schema.control === 'rotation') return [0, 0, 0, 1]
    if (schema.type === 'array') return schema.prefixItems ? schema.prefixItems.map(item => typeof item === 'boolean' ? null : getInitialFieldValue(item, world) ?? null) : []
    if (schema.properties) {
        const valueOfField: { [field: string]: EditorValue } = {}
        for (const [field, childSchema] of Object.entries(schema.properties)) {
            if (typeof childSchema === 'boolean') continue
            const value = childSchema.anyOf?.some(option => option.type === 'null') ? null : getInitialFieldValue(childSchema, world)
            if (value === undefined) return undefined
            valueOfField[field] = value
        }
        return valueOfField
    }
    return undefined
}
export const isReferenceTarget = (schema: EditorFieldSchema, world: StoredWorld, object: LiveObject) => schema.objectKind === 'physical' ? Object.hasOwn(world.physicsInfo3DOfId, object.id) : schema.objectKind === '' || getObjectKind(object) === schema.objectKind
