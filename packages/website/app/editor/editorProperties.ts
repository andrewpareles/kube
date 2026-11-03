import { z } from 'zod'
import { fieldSchemaOfKind, getObjectKind, getFieldLabel, physicsInfo3DSchema, physicsToggles3DSchema, primitiveSchemaOfKind, type EditorFieldSchema, type EditorValue, type StoredWorld } from './editorObjectSchema.ts'
import type { EditorClientDelta, EditorSection } from './editorSync.ts'

// The inspector intersects schemas while retaining each object's storage path and current value.
export type EditorPropertyUpdate = EditorValue | ((value: EditorValue, schema: EditorFieldSchema) => EditorValue)
type PropertyTarget = { objectId: number, section: EditorSection, path: string[], schema: EditorFieldSchema, value: EditorValue }
const locationFieldSchema = z.toJSONSchema(physicsInfo3DSchema.shape.location)
const movementFieldSchema = z.toJSONSchema(physicsInfo3DSchema.shape.movement)
const togglesFieldSchema = z.toJSONSchema(physicsToggles3DSchema)
const primitiveFieldSchemaOfKind = Object.fromEntries(Object.entries(primitiveSchemaOfKind).map(([kind, schema]) => [kind, z.toJSONSchema(schema)]))

export const areEditorValuesEqual = (left: unknown, right: unknown): boolean => {
    if (left === right) return true
    if (left === null || right === null || typeof left !== 'object' || typeof right !== 'object') return false
    if (Array.isArray(left) || Array.isArray(right)) return Array.isArray(left) && Array.isArray(right) && left.length === right.length && left.every((value, index) => areEditorValuesEqual(value, right[index]))
    const leftEntries = Object.entries(left), rightEntries = Object.entries(right)
    return leftEntries.length === rightEntries.length && leftEntries.every(([key, value]) => rightEntries.some(([otherKey, otherValue]) => key === otherKey && areEditorValuesEqual(value, otherValue)))
}

// Nested objects can share fields even when their complete schemas differ, such as mesh and brick materials.
const getCommonSchema = (schemas: EditorFieldSchema[]): EditorFieldSchema | null => {
    const first = schemas[0]
    if (schemas.every(schema => areEditorValuesEqual(first, schema))) return first
    if (schemas.every(schema => schema.anyOf?.some(option => option.type === 'null'))) {
        const inners = schemas.flatMap(schema => schema.anyOf?.filter(option => option.type !== 'null') ?? [])
        if (inners.length !== schemas.length) return null
        const common = getCommonSchema(inners)
        return common ? { anyOf: [common, { type: 'null' }] } : null
    }
    if (!schemas.every(schema => schema.type === 'object' && schema.properties && schema.control === first.control)) return null
    const schemaOfField: { [field: string]: EditorFieldSchema } = {}
    for (const field of Object.keys(first.properties ?? {})) {
        const children = schemas.map(schema => schema.properties?.[field])
        if (!children.every(child => child !== undefined && typeof child !== 'boolean')) continue
        const common = getCommonSchema(children)
        if (common) schemaOfField[field] = common
    }
    return Object.keys(schemaOfField).length ? { ...first, properties: schemaOfField, required: Object.keys(schemaOfField) } : null
}

export const getEditorPropertySections = (world: StoredWorld, objectIds: number[]) => {
    const kinds = objectIds.map(id => getObjectKind(world.objectOfId[id]))
    const objectSections = objectIds.map(objectId => {
        const object = world.objectOfId[objectId], kind = getObjectKind(object)
        const sections: { label: string, target: PropertyTarget }[] = []
        const add = (label: string, section: EditorSection, path: string[], schema: EditorFieldSchema, value: EditorValue) => sections.push({ label, target: { objectId, section, path, schema, value } })
        add('Name', 'objectOfId', ['name'], { type: 'string' }, object.name)
        if (Object.keys(fieldSchemaOfKind[kind]?.properties ?? {}).length) add('Object', 'objectOfId', ['info', kind], fieldSchemaOfKind[kind], object.info[kind])
        if (world.primitiveSettingsOfId[objectId]) add('Shape', 'primitiveSettingsOfId', [kind], primitiveFieldSchemaOfKind[kind], world.primitiveSettingsOfId[objectId][kind])
        // Present transforms and motion directly instead of repeating the storage hierarchy in the UI.
        const physics = world.physicsInfo3DOfId[objectId]
        if (physics) {
            add('Transform', 'physicsInfo3DOfId', ['location'], locationFieldSchema, physics.location)
            add('Movement', 'physicsInfo3DOfId', ['movement'], movementFieldSchema, physics.movement)
        }
        if (world.physicsToggles3DOfId[objectId]) add('Physics', 'physicsToggles3DOfId', [], togglesFieldSchema, world.physicsToggles3DOfId[objectId])
        return sections
    })
    return (objectSections[0] ?? []).flatMap(section => {
        const targets = objectSections.flatMap(sections => sections.filter(other => other.label === section.label).map(other => other.target))
        if (targets.length !== objectIds.length) return []
        const schema = getCommonSchema(targets.map(target => target.schema))
        if (!schema) return []
        const label = section.label === 'Object' ? kinds.every(kind => kind === kinds[0]) ? getFieldLabel(kinds[0]) : 'Object' : section.label
        return [{ label, schema, targets }]
    })
}

// Resolve a leaf separately for each object, so updating one component never copies its siblings.
export const getEditorPropertyEdits = (targets: PropertyTarget[], path: string[], update: EditorPropertyUpdate): EditorClientDelta[] => targets.map(target => {
    let value = target.value, schema = target.schema
    for (const field of path) {
        if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error('This property no longer exists.')
        const options = schema.anyOf
        if (options) schema = options.find(option => option.type === 'object' && Object.hasOwn(option.properties ?? {}, field)) ?? schema
        const childSchema = schema.properties?.[field]
        if (!childSchema || typeof childSchema === 'boolean') throw new Error('This property no longer exists.')
        value = value[field]; schema = childSchema
    }
    return { objectId: target.objectId, section: target.section, path: [...target.path, ...path], value: typeof update === 'function' ? update(value, schema) : update }
})
