import assert from 'node:assert/strict'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import test from 'node:test'
import { createEditorWorldState, editorWorldReducer } from '../packages/website/app/editor/editorWorld.ts'
import { getEditorPropertyEdits, getEditorPropertySections } from '../packages/website/app/editor/editorProperties.ts'
import { fieldSchemaOfKind, getInitialFieldValue, getObjectKind, storedWorldSchema } from '../packages/website/app/editor/editorObjectSchema.ts'
import { createEditorChatMessage } from '../packages/website/app/editor/editorChat.ts'
import { repoPath } from './testPaths.ts'
import { withIsPushable } from './authoredWorld.ts'

// This authored fixture predates glow on every object's visual settings; the editor server fills it in today.
const loadWorld = () => {
    const world = storedWorldSchema.parse(withIsPushable(JSON.parse(fs.readFileSync(repoPath('game-35-latest.kube'), 'utf8'))))
    for (const object of Object.values(world.objectOfId))
        for (const [kind, info] of Object.entries(object.info))
            if (info && typeof info === 'object' && !Array.isArray(info) && 'visualSettings' in info && info.visualSettings && typeof info.visualSettings === 'object') object.info[kind] = { ...info, visualSettings: { ...info.visualSettings, glow: 0 } }
    return world
}

test('selection toggles, survives refresh, and resets on an ordinary click', () => {
    const world = loadWorld()
    const [first, second, third] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    let state = createEditorWorldState(world)
    for (const object of [first, second, third]) state = editorWorldReducer(state, { type: 'toggle-select', objectId: object.id })
    assert.deepEqual(state.selectedObjectIds, [first.id, second.id, third.id])
    state = editorWorldReducer(state, { type: 'toggle-select', objectId: second.id })
    assert.deepEqual(state.selectedObjectIds, [first.id, third.id])
    state = editorWorldReducer(state, { type: 'toggle-pick', objectId: null })
    assert.deepEqual(state.selectedObjectIds, [first.id, third.id])
    const incoming = structuredClone(world)
    delete incoming.objectOfId[first.id]
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.deepEqual(state.selectedObjectIds, [third.id])
    state = editorWorldReducer(state, { type: 'select', objectId: second.id })
    assert.deepEqual(state.selectedObjectIds, [second.id])
    state = editorWorldReducer(state, { type: 'pick', objectId: null })
    assert.deepEqual(state.selectedObjectIds, [])
})

test('an undo selects what it changed, including an object whose return arrives afterwards', () => {
    const world = loadWorld()
    const [first, second, third] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    const withoutThird = structuredClone(world)
    delete withoutThird.objectOfId[third.id]
    let state = editorWorldReducer(createEditorWorldState(withoutThird), { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'select-objects', objectIds: [second.id, third.id] })
    assert.deepEqual(state.selectedObjectIds, [second.id])
    state = editorWorldReducer(state, { type: 'refresh', world })
    assert.deepEqual(state.selectedObjectIds, [second.id, third.id])
    assert.deepEqual(state.pendingSelectedObjectIds, [])
    state = editorWorldReducer(createEditorWorldState(withoutThird), { type: 'select-objects', objectIds: [third.id] })
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'refresh', world })
    assert.deepEqual(state.selectedObjectIds, [first.id], 'A click after the undo replaces what it was waiting for')
})

test('a server world older than what was typed does not take the typing back', () => {
    const world = loadWorld()
    const [ball] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    const rename = (state: ReturnType<typeof createEditorWorldState>, value: string) => editorWorldReducer(state, { type: 'edit', description: 'Name on Ball', edits: [{ objectId: ball.id, section: 'objectOfId', path: ['name'], value }] })
    const serverWorldNamed = (name: string) => ({ ...world, objectOfId: { ...world.objectOfId, [ball.id]: { ...ball, name } } })
    let state = rename(createEditorWorldState(world), 'Xa')
    const typedA = state.unackedActions[0].action
    state = rename(state, 'Xab')
    const typedB = state.unackedActions[1].action
    state = editorWorldReducer(state, { type: 'refresh', world: serverWorldNamed('Xa'), seq: 1 })
    assert.equal(state.world.objectOfId[ball.id].name, 'Xab', 'The echo of the first keystroke keeps the second')
    state = editorWorldReducer(state, { type: 'acked', action: typedA, seq: 1 })
    state = editorWorldReducer(state, { type: 'acked', action: typedB, seq: 3 })
    state = editorWorldReducer(state, { type: 'refresh', world: serverWorldNamed('Xa'), seq: 2 })
    assert.equal(state.world.objectOfId[ball.id].name, 'Xab', 'A batch sent before the edit was applied cannot take it back, even after it was acknowledged')
    state = editorWorldReducer(state, { type: 'refresh', world: serverWorldNamed('Theirs'), seq: 3 })
    assert.equal(state.world.objectOfId[ball.id].name, 'Theirs', 'Once the batch that includes the edit arrives, the server decides')
    assert.deepEqual(state.unackedActions, [])
})

test('an edit acknowledged after its batch already arrived stops covering the server world at once', () => {
    const world = loadWorld()
    const [ball] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit', description: 'Name', edits: [{ objectId: ball.id, section: 'objectOfId', path: ['name'], value: 'Mine' }] })
    const mine = state.unackedActions[0].action
    state = editorWorldReducer(state, { type: 'refresh', world: { ...world, objectOfId: { ...world.objectOfId, [ball.id]: { ...ball, name: 'Theirs' } } }, seq: 4 })
    assert.equal(state.world.objectOfId[ball.id].name, 'Mine')
    state = editorWorldReducer(state, { type: 'acked', action: mine, seq: 4 })
    assert.equal(state.world.objectOfId[ball.id].name, 'Theirs')
})

test('a rejected edit leaves the other unacknowledged edits in place', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit', description: 'Name', edits: [{ objectId: first.id, section: 'objectOfId', path: ['name'], value: 'Rejected' }] })
    const rejected = state.unackedActions[0].action
    state = editorWorldReducer(state, { type: 'edit', description: 'Name', edits: [{ objectId: second.id, section: 'objectOfId', path: ['name'], value: 'Kept' }] })
    state = editorWorldReducer(state, { type: 'rejected', action: rejected })
    assert.equal(state.world.objectOfId[first.id].name, first.name)
    assert.equal(state.world.objectOfId[second.id].name, 'Kept')
})

test('the name preview follows the editor server rules', () => {
    const world = loadWorld()
    const [ball] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    const rename = (value: string) => editorWorldReducer(createEditorWorldState(world), { type: 'edit', description: 'Name', edits: [{ objectId: ball.id, section: 'objectOfId', path: ['name'], value }] }).world.objectOfId[ball.id].name
    assert.equal(rename('red lamp!'), 'red-lamp-')
    assert.equal(rename(''), '-')
    assert.equal(rename('x'.repeat(70)).length, 64)
})

test('modifier viewport picks toggle groups and can select siblings inside an opened group', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    const groupId = Math.max(...Object.values(world.objectOfId).map(object => object.id)) + 1
    world.objectOfId[groupId] = { id: groupId, name: 'Selection group', info: { RigidGroup: { followSkeletonId: null } } }
    for (const [id, children] of Object.entries(world.childrenOfId)) world.childrenOfId[id] = children.filter(child => child !== first.id && child !== second.id)
    world.childrenOfId[groupId] = [first.id, second.id]
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'pick', objectId: first.id })
    assert.deepEqual(state.selectedObjectIds, [groupId])
    state = editorWorldReducer(state, { type: 'toggle-pick', objectId: second.id })
    assert.deepEqual(state.selectedObjectIds, [])
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'toggle-pick', objectId: second.id })
    assert.deepEqual(state.selectedObjectIds, [first.id, second.id])
    state = editorWorldReducer(state, { type: 'toggle-pick', objectId: first.id })
    assert.deepEqual(state.selectedObjectIds, [second.id])
})

test('multi-selecting scripts leaves the world visible and retains open drafts', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Script)
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: 'const draft = 1' })
    state = editorWorldReducer(state, { type: 'toggle-select', objectId: second.id })
    assert.deepEqual(state.selectedObjectIds, [first.id, second.id])
    assert.equal(state.activeScriptId, null)
    assert.deepEqual(state.openScriptIds, [first.id])
    assert.equal(state.scriptDraftOfId[first.id], 'const draft = 1')
})

test('common schemas include nested fields across types and retain the correct paths for absolute edits', () => {
    const world = loadWorld()
    const [brick, ball] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    ball.info = { Ball: structuredClone(brick.info.Brick) }
    world.primitiveSettingsOfId[ball.id] = { Ball: { radius: 2 } }
    const ids = [brick.id, ball.id]
    const sections = getEditorPropertySections(world, ids)
    assert.ok(!sections.some(section => section.label === 'Shape'), 'A brick size and a ball radius are not shared')
    const info = sections.find(section => section.label === 'Object')
    assert.ok(info?.schema.properties?.visualSettings)
    const edits = getEditorPropertyEdits(info.targets, ['visualSettings', 'opacity'], 0.25)
    assert.deepEqual(edits.map(edit => edit.path), [['info', 'Brick', 'visualSettings', 'opacity'], ['info', 'Ball', 'visualSettings', 'opacity']])
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits })
    assert.equal(state.error, null)
    for (const id of ids) assert.equal(Object.values(state.world.objectOfId[id].info)[0].visualSettings.opacity, 0.25)
    const script = Object.values(world.objectOfId).find(object => object.info.Script)
    assert.ok(script)
    assert.deepEqual(getEditorPropertySections(world, [brick.id, script.id]).map(section => section.label), ['Name'])
})

test('component edits set one absolute value and preserve every other component', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    world.physicsInfo3DOfId[first.id].location.position = [1, 2, 3]
    world.physicsInfo3DOfId[second.id].location.position = [11, 12, 13]
    const transform = getEditorPropertySections(world, [first.id, second.id]).find(section => section.label === 'Transform')
    assert.ok(transform)
    const edits = getEditorPropertyEdits(transform.targets, ['position'], value => {
        assert.ok(Array.isArray(value))
        return value.map((component, index) => index === 0 ? 7 : component)
    })
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits })
    assert.deepEqual(state.world.physicsInfo3DOfId[first.id].location.position, [7, 2, 3])
    assert.deepEqual(state.world.physicsInfo3DOfId[second.id].location.position, [7, 12, 13])
    assert.deepEqual(world.physicsInfo3DOfId[first.id].location.position, [1, 2, 3])
})

test('enabling a shared optional object initializes each full schema and preserves existing values', () => {
    const world = loadWorld()
    const [brick, mesh] = Object.values(world.objectOfId).filter(object => object.info.Brick)
    const meshInfo = getInitialFieldValue(fieldSchemaOfKind.Mesh, world)
    assert.ok(meshInfo && typeof meshInfo === 'object' && !Array.isArray(meshInfo))
    mesh.info = { Mesh: meshInfo }
    delete world.primitiveSettingsOfId[mesh.id]
    brick.info.Brick.material = null
    mesh.info.Mesh.material = null
    const section = getEditorPropertySections(world, [brick.id, mesh.id]).find(section => section.label === 'Object')
    assert.ok(section?.schema.properties?.material)
    const edits = getEditorPropertyEdits(section.targets, ['material'], (value, schema) => {
        const inner = schema.anyOf?.find(option => option.type !== 'null')
        assert.ok(inner)
        return value ?? getInitialFieldValue(inner, world) ?? null
    })
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits })
    assert.equal(state.error, null)
    assert.equal(state.world.objectOfId[brick.id].info.Brick.material.tileSizeU, 1)
    assert.deepEqual(Object.keys(state.world.objectOfId[mesh.id].info.Mesh.material), ['materialInfoId'])
})

test('chat messages carry every selected object through AI SDK validation', async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { validateUIMessages } = require('ai')
    const world = loadWorld()
    const objects = Object.values(world.objectOfId).slice(0, 3)
    const message = createEditorChatMessage('Make these blue', objects)
    const originalName = objects[0].name
    objects[0].name = 'Changed after sending'
    const [validated] = await validateUIMessages({ messages: [message] })
    assert.deepEqual(validated.parts[0], { type: 'text', text: 'Make these blue' })
    assert.equal(validated.parts.length, 2)
    assert.equal(validated.parts[1].type, 'data-selected-objects')
    assert.deepEqual(validated.parts[1].data, objects.map((object, index) => ({ objectId: object.id, name: index === 0 ? originalName : object.name, kind: getObjectKind(object) })))
    assert.equal(createEditorChatMessage('No selection', []).parts.length, 1)
})
