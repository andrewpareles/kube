import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { createEditorWorldState, editorWorldReducer, getExplorerRows, type EditorWorldState } from '../packages/website/app/editor/editorWorld.ts'
import { fieldSchemaOfKind, getInitialFieldValue, objectInfoSchemaOfKind, storedWorldSchema, type EditorFieldSchema } from '../packages/website/app/editor/editorObjectSchema.ts'
import { degreesToQuaternion, quaternionToDegrees } from '../packages/website/app/editor/editorRotation.ts'
import { repoPath } from './testPaths.ts'
import { withIsPushable } from './authoredWorld.ts'
import type { EditorClientDelta } from '../packages/website/app/editor/editorSync.ts'
import { getChangedTextRange, getScriptSnapshots } from '../packages/website/app/editor/editorScripts.ts'

// Use an authored world to exercise the actual serialized structure instead of a second test model.
// This authored fixture predates glow on every object's visual settings; the editor server fills it in today.
const loadWorld = () => {
    const world = storedWorldSchema.parse(withIsPushable(JSON.parse(fs.readFileSync(repoPath('game-35-latest.kube'), 'utf8'))))
    for (const object of Object.values(world.objectOfId))
        for (const [kind, info] of Object.entries(object.info))
            if (info && typeof info === 'object' && !Array.isArray(info) && 'visualSettings' in info && info.visualSettings && typeof info.visualSettings === 'object') object.info[kind] = { ...info, visualSettings: { ...info.visualSettings, glow: 0 } }
    return world
}

test('script selection opens unique tabs and closing or returning to World preserves drafts', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Script)
    assert.ok(first && second)
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    assert.deepEqual(state.openScriptIds, [first.id])
    assert.equal(state.activeScriptId, first.id)
    state = editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: 'const local = 1' })
    assert.equal(state.world, world, 'Typing must not change the world sent to the renderer')
    state = editorWorldReducer(state, { type: 'select', objectId: second.id })
    assert.deepEqual(state.openScriptIds, [first.id, second.id])
    state = editorWorldReducer(state, { type: 'show-world' })
    assert.equal(state.activeScriptId, null)
    assert.equal((state.selectedObjectIds.at(-1) ?? null), second.id)
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    state = editorWorldReducer(state, { type: 'close-script', objectId: first.id })
    assert.equal(state.activeScriptId, second.id)
    assert.equal((state.selectedObjectIds.at(-1) ?? null), second.id)
    state = editorWorldReducer(state, { type: 'close-script', objectId: second.id })
    assert.equal(state.activeScriptId, null)
    assert.deepEqual(state.openScriptIds, [])
    state = editorWorldReducer(state, { type: 'select', objectId: first.id })
    assert.equal(state.scriptDraftOfId[first.id], 'const local = 1')
    state = editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: '' })
    assert.equal(state.scriptDraftOfId[first.id], '', 'An empty script is an editable draft')
    state = editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: world.scriptRawCodeOfId[first.id] })
    assert.equal(state.scriptDraftOfId[first.id], undefined, 'Returning to saved text clears the local change indicator')
})

test('script refreshes preserve local text and discard tabs and drafts for removed or changed objects', () => {
    const world = loadWorld()
    const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Script)
    assert.ok(first && second)
    let state = createEditorWorldState(world)
    for (const object of [first, second]) state = editorWorldReducer(state, { type: 'select', objectId: object.id })
    state = editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: 'const draft = 1' })
    const incoming = structuredClone(world)
    incoming.scriptRawCodeOfId[first.id] = 'const remote = 2'
    incoming.scriptRawCodeOfId[second.id] = 'const untouched = 3'
    incoming.objectOfId[first.id].name = 'Renamed script'
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.equal(state.scriptDraftOfId[first.id], 'const draft = 1')
    assert.equal(state.scriptDraftOfId[second.id], undefined)
    assert.equal(state.world.scriptRawCodeOfId[second.id], 'const untouched = 3')
    assert.equal(state.world.objectOfId[first.id].name, 'Renamed script')
    const removed = structuredClone(incoming)
    delete removed.objectOfId[second.id]
    state = editorWorldReducer(state, { type: 'refresh', world: removed })
    assert.deepEqual(state.openScriptIds, [first.id])
    assert.equal(state.activeScriptId, null)
    removed.objectOfId[first.id].info = { LocalGroup: {} }
    state = editorWorldReducer(state, { type: 'refresh', world: removed })
    assert.deepEqual(state.openScriptIds, [])
    assert.deepEqual(state.scriptDraftOfId, {})
    assert.equal(editorWorldReducer(state, { type: 'edit-script', objectId: first.id, text: 'stale edit' }), state)
})

test('a refresh keeps newer local script text and clears drafts the server has caught up to', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])

    // Typing continues while an edit is in flight, so a refresh carrying the older text must not clobber it.
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'const newer = 2;' })
    state = editorWorldReducer(state, { type: 'refresh', world: structuredClone(world) })
    assert.equal(state.scriptDraftOfId[objectId], 'const newer = 2;')
    assert.equal(state.world.scriptRawCodeOfId[objectId], world.scriptRawCodeOfId[objectId])

    // Once the server echoes that text back there is nothing local left to keep.
    const incoming = { ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: 'const newer = 2;' } }
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.equal(state.scriptDraftOfId[objectId], undefined)
    assert.equal(state.world.scriptRawCodeOfId[objectId], 'const newer = 2;')

    // Typing the saved text again is just another edit, so it needs no draft of its own.
    state = editorWorldReducer(state, { type: 'edit-script', objectId, text: 'const newer = 2;' })
    assert.equal(state.scriptDraftOfId[objectId], undefined)
})

const getShownScriptText = (state: EditorWorldState, objectId: number) => state.scriptDraftOfId[objectId] ?? state.world.scriptRawCodeOfId[objectId]

test('the batch that includes a script save shows the server text, even when the AI rewrote the script after that save', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])
    const rewritten = { ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: 'const ai = 2;' } }

    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'const mine = 1;' })
    state = editorWorldReducer(state, { type: 'acked', action: state.actionsToSend[0], seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'const mine = 1;')
    state = editorWorldReducer(state, { type: 'refresh', world: rewritten, seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'const ai = 2;')

    // The server's answer can also arrive after the batch that includes the save.
    state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'const mine = 1;' })
    state = editorWorldReducer(state, { type: 'refresh', world: rewritten, seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'const mine = 1;', 'Until the server answers, the save may still be on its way')
    state = editorWorldReducer(state, { type: 'acked', action: state.actionsToSend[0], seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'const ai = 2;')
})

test('a rejected script save puts the server text back, so later changes to that script show', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'const mine = 1;' })
    state = editorWorldReducer(state, { type: 'rejected', action: state.actionsToSend[0] })
    assert.equal(getShownScriptText(state, objectId), world.scriptRawCodeOfId[objectId])
    state = editorWorldReducer(state, { type: 'refresh', world: { ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: 'const ai = 2;' } }, seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'const ai = 2;')
})

test('when the server refuses only some script saves, the shown text is the newest save it has not refused', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])
    const withScript = (text: string) => ({ ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: text } })

    // A pausing editor refuses the save it was handling while the next one is still on its way.
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'x' })
    state = editorWorldReducer(state, { type: 'edit-script', objectId, text: 'xy' })
    const [firstSave, secondSave] = state.actionsToSend
    state = editorWorldReducer(state, { type: 'rejected', action: firstSave })
    assert.equal(getShownScriptText(state, objectId), 'xy')
    state = editorWorldReducer(state, { type: 'acked', action: secondSave, seq: 1 })
    state = editorWorldReducer(state, { type: 'refresh', world: withScript('xy'), seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'xy')
    assert.equal(state.scriptDraftOfId[objectId], undefined)

    // Refusing the newer save falls back to the older one the server took, not to the text before both.
    state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'x' })
    state = editorWorldReducer(state, { type: 'edit-script', objectId, text: 'xy' })
    const [takenSave, refusedSave] = state.actionsToSend
    state = editorWorldReducer(state, { type: 'acked', action: takenSave, seq: 1 })
    state = editorWorldReducer(state, { type: 'rejected', action: refusedSave })
    assert.equal(getShownScriptText(state, objectId), 'x')
    state = editorWorldReducer(state, { type: 'refresh', world: withScript('x'), seq: 1 })
    assert.equal(getShownScriptText(state, objectId), 'x')
    assert.deepEqual(state.actionsToSend, [])
})

test('text typed after a script save stays on screen when the batch with that save arrives', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])
    const saved = world.scriptRawCodeOfId[objectId]

    // Typing the saved text back while the first save is on its way must not show the first save's text.
    let state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: 'const typed = 1;' })
    const firstSave = state.actionsToSend[0]
    state = editorWorldReducer(state, { type: 'edit-script', objectId, text: saved })
    state = editorWorldReducer(state, { type: 'acked', action: firstSave, seq: 1 })
    state = editorWorldReducer(state, { type: 'refresh', world: { ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: 'const typed = 1;' } }, seq: 1 })
    assert.equal(getShownScriptText(state, objectId), saved)
})

test('diagnostic snapshots contain the full hierarchy and unsaved text, including closed script drafts', () => {
    const world = loadWorld()
    const objectId = Number(Object.keys(world.scriptRawCodeOfId)[0])
    const state = editorWorldReducer(createEditorWorldState(world), { type: 'edit-script', objectId, text: '' })
    const scripts = getScriptSnapshots(state)
    assert.equal(scripts.length, Object.keys(world.scriptRawCodeOfId).length)
    const script = scripts.find(script => script.objectId === objectId)
    assert.ok(script)
    assert.equal(script.text, '')
    assert.ok(script.path.startsWith(`${world.objectOfId[world.rootObjectId].name}/`))
    assert.ok(script.path.endsWith(`/${world.objectOfId[objectId].name}`))
    assert.deepEqual(state.openScriptIds, [])
})

test('script text from the server replaces only the stretch that changed, and never half of an emoji', () => {
    assert.deepEqual(getChangedTextRange('const a = 1;\nconst b = 2;', 'const a = 1;\nconst b = 3;'), { start: 23, end: 24, replacement: '3' })
    assert.deepEqual(getChangedTextRange('aa', 'aaa'), { start: 2, end: 2, replacement: 'a' })
    assert.deepEqual(getChangedTextRange('aaa', 'aa'), { start: 2, end: 3, replacement: '' })
    assert.deepEqual(getChangedTextRange('x😀y', 'x😁y'), { start: 1, end: 3, replacement: '😁' })
    assert.deepEqual(getChangedTextRange('😀', '😁😀'), { start: 0, end: 2, replacement: '😁😀' })
    const pairs: [string, string][] = [['', 'new'], ['old', ''], ['same', 'same'], ['a😀b😀c', 'a😀c'], ['🙂🙃', '🙃🙂']]
    for (const [oldText, newText] of pairs) {
        const { start, end, replacement } = getChangedTextRange(oldText, newText)
        assert.equal(oldText.slice(0, start) + replacement + oldText.slice(end), newText)
    }
})

// Wrap an authored rigid group in another rigid group, with a local folder between them.
const loadGroupedWorld = () => {
    const world = loadWorld()
    const group = Object.values(world.objectOfId).find(object => object.info.RigidGroup)
    assert.ok(group)
    const bricks = world.childrenOfId[group.id].map(id => world.objectOfId[id]).filter(object => object.info.Brick)
    assert.ok(bricks.length >= 2)
    const outerId = Math.max(...Object.values(world.objectOfId).map(object => object.id)) + 1
    const folderId = outerId + 1
    for (const [parent, children] of Object.entries(world.childrenOfId)) world.childrenOfId[parent] = children.filter(id => id !== group.id)
    world.objectOfId[outerId] = { id: outerId, name: 'Outer rigid group', info: { RigidGroup: { followSkeletonId: null } } }
    world.objectOfId[folderId] = { id: folderId, name: 'Local folder', info: { LocalGroup: {} } }
    world.childrenOfId[world.rootObjectId].push(outerId)
    world.childrenOfId[outerId] = [folderId]
    world.childrenOfId[folderId] = [group.id]
    return { world, outerId, group, bricks }
}

test('viewport picking enters nested rigid groups and stays inside them when picking siblings', () => {
    const { world, outerId, group, bricks: [left, right] } = loadGroupedWorld()
    const original = structuredClone(world)
    let state = createEditorWorldState(world)
    for (const [hit, selected] of [[left.id, outerId], [left.id, group.id], [left.id, left.id], [left.id, left.id], [right.id, right.id]]) {
        state = editorWorldReducer(state, { type: 'pick', objectId: hit })
        assert.equal((state.selectedObjectIds.at(-1) ?? null), selected)
        assert.equal(state.world, world)
        assert.equal(state.error, null)
    }
    const outside = Object.values(world.objectOfId).find(object => object.info.Brick && !world.childrenOfId[group.id].includes(object.id))
    assert.ok(outside)
    state = editorWorldReducer(state, { type: 'pick', objectId: outside.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), outside.id, 'Local groups alone must not intercept picking')
    state = editorWorldReducer(state, { type: 'pick', objectId: right.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), outerId, 'Entering an unrelated group starts at its outermost rigid group')
    for (const hit of [null, Number.MAX_SAFE_INTEGER]) {
        state = editorWorldReducer(state, { type: 'pick', objectId: hit })
        assert.equal((state.selectedObjectIds.at(-1) ?? null), null)
        state = editorWorldReducer(state, { type: 'pick', objectId: left.id })
        assert.equal((state.selectedObjectIds.at(-1) ?? null), outerId, 'Empty or stale hits reset drilling')
    }
    assert.deepEqual(world, original)
})

test('exact selection and refreshed hierarchy determine where the next viewport click drills', () => {
    const { world, outerId, group, bricks: [left, right] } = loadGroupedWorld()
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'select', objectId: left.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), left.id, 'Explorer and references select the exact object immediately')
    state = editorWorldReducer(state, { type: 'pick', objectId: right.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), right.id)
    state = editorWorldReducer(state, { type: 'select', objectId: outerId })
    state = editorWorldReducer(state, { type: 'pick', objectId: left.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), group.id)
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [{ objectId: group.id, section: 'objectOfId', path: ['name'], value: 'Edited-group' }] })
    state = editorWorldReducer(state, { type: 'pick', objectId: left.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), left.id, 'Editing properties must not reset drilling')
    assert.equal(state.world.objectOfId[group.id].name, 'Edited-group')
    const incoming = structuredClone(world)
    incoming.childrenOfId[group.id] = incoming.childrenOfId[group.id].filter(id => id !== left.id)
    incoming.childrenOfId[world.rootObjectId].push(left.id)
    for (const { action } of state.unackedActions) state = editorWorldReducer(state, { type: 'acked', action, seq: 1 })
    state = editorWorldReducer(state, { type: 'refresh', world: incoming, seq: 1 })
    state = editorWorldReducer(state, { type: 'select', objectId: null })
    state = editorWorldReducer(state, { type: 'pick', objectId: left.id })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), left.id, 'Picking must use the latest hierarchy after reparenting')
    assert.equal(state.world.objectOfId[group.id].name, incoming.objectOfId[group.id].name)
})

test('inspector schemas cover every Rust object variant and its exact Info fields', () => {
    const rust = fs.readFileSync(repoPath('crates/game_types/src/object_types.rs'), 'utf8')
    const variants = [...rust.slice(rust.indexOf('pub enum ObjectInfo {'), rust.indexOf('macro_rules! impl_from_object_info')).matchAll(/^    (\w+)\((\w+Info)\),$/gm)]
    assert.deepEqual(Object.keys(objectInfoSchemaOfKind).sort(), variants.map(match => match[1]).sort())
    const structs = fs.readdirSync(repoPath('crates/game_types/src/object_types')).filter(file => file.endsWith('.rs')).map(file => fs.readFileSync(repoPath('crates/game_types/src/object_types', file), 'utf8')).join('\n')
    for (const [, kind, name] of variants) {
        const body = structs.match(new RegExp(`pub struct ${name} \\{([^}]*?)\\}`))?.[1]
        assert.notEqual(body, undefined, name)
        const fields = [...(body ?? '').matchAll(/^\s*pub (\w+):/gm)].map(match => match[1]).sort()
        assert.deepEqual(Object.keys(objectInfoSchemaOfKind[kind].shape).sort(), fields, name)
    }
})

test('nested schema fields keep matching the Rust storage structs', () => {
    const structs = ['shared', 'brick', 'mesh', 'clock', 'player'].map(file => fs.readFileSync(repoPath(`crates/game_types/src/object_types/${file}.rs`), 'utf8')).join('\n')
    const check = (name: string, schema: EditorFieldSchema) => {
        const body = structs.match(new RegExp(`pub struct ${name} \\{([^}]*?)\\}`))?.[1]
        assert.notEqual(body, undefined, name)
        assert.deepEqual(Object.keys(schema.properties ?? {}).sort(), [...(body ?? '').matchAll(/^\s*pub (\w+):/gm)].map(match => match[1]).sort(), name)
    }
    const properties = fieldSchemaOfKind.Brick.properties
    assert.ok(properties && typeof properties.visualSettings === 'object' && typeof properties.material === 'object')
    check('VisualSettings', properties.visualSettings)
    check('BrickMaterialSettings', properties.material.anyOf![0])
    const color = properties.visualSettings.properties?.color
    assert.ok(typeof color === 'object')
    check('Color3', color)
})

test('name and nested edits are immutable, affect only the target field, and validate before rendering', () => {
    const world = loadWorld()
    const brick = Object.values(world.objectOfId).find(object => object.info.Brick)
    assert.ok(brick)
    const original = structuredClone(world)
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'select', objectId: brick.id })
    const edit: EditorClientDelta = { objectId: brick.id, section: 'objectOfId', path: ['info', 'Brick', 'visualSettings', 'opacity'], value: 0.25 }
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [edit] })
    assert.equal(state.error, null)
    assert.equal((state.selectedObjectIds.at(-1) ?? null), brick.id)
    assert.deepEqual(world, original)
    const info = objectInfoSchemaOfKind.Brick.parse(state.world.objectOfId[brick.id].info.Brick)
    assert.equal(info.visualSettings.opacity, 0.25)
    assert.equal(state.world.childrenOfId, world.childrenOfId)
    assert.equal(state.world.objectOfId[world.rootObjectId], world.objectOfId[world.rootObjectId])
    for (const value of [NaN, Infinity, -1, 1.1, 'text']) {
        const rejected = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [{ ...edit, value }] })
        assert.ok(rejected.error)
        assert.equal(rejected.world, state.world)
    }
    const forbiddenEdits: EditorClientDelta[] = [
        { ...edit, path: ['id'], value: 999 },
        { ...edit, path: ['info', 'Script', 'objectLocation'], value: 'Client' },
        { ...edit, section: 'scriptRawCodeOfId', path: [], value: 'const invalid = true;' },
    ]
    for (const edit of forbiddenEdits) {
        const forbidden = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [edit] })
        assert.ok(forbidden.error)
        assert.equal(forbidden.world, state.world)
    }
})

test('server refreshes keep unacknowledged previews, replace acknowledged ones, and clear deleted selections', () => {
    const world = loadWorld()
    const id = world.rootObjectId
    let state = createEditorWorldState(world)
    state = editorWorldReducer(state, { type: 'select', objectId: id })
    const edit: EditorClientDelta = { objectId: id, section: 'objectOfId', path: ['name'], value: 'Local-name' }
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [edit] })
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [{ ...edit, value: 'Latest-name' }] })
    const incoming = structuredClone(world)
    incoming.objectOfId[id].name = 'Remote name'
    incoming.scriptRawCodeOfId['12345'] = 'const updated = true'
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.equal(state.world.objectOfId[id].name, 'Latest-name', 'Edits the server has not applied yet stay on top')
    for (const { action } of state.unackedActions) state = editorWorldReducer(state, { type: 'acked', action, seq: 1 })
    state = editorWorldReducer(state, { type: 'refresh', world: incoming, seq: 1 })
    assert.equal(state.world.objectOfId[id].name, 'Remote name')
    assert.equal(state.world.scriptRawCodeOfId['12345'], 'const updated = true')
    assert.equal(incoming.objectOfId[id].name, 'Remote name')
    delete incoming.objectOfId[id]
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.equal((state.selectedObjectIds.at(-1) ?? null), null)
})

test('optional fields update previews and server refreshes replace them once acknowledged', () => {
    const world = loadWorld()
    const brick = Object.values(world.objectOfId).find(object => object.info.Brick)
    const material = Object.values(world.objectOfId).find(object => object.info.Material)
    assert.ok(brick && material)
    let state = createEditorWorldState(world)
    const edit: EditorClientDelta = { objectId: brick.id, section: 'objectOfId', path: ['info', 'Brick', 'material'], value: { materialInfoId: material.id, tileSizeU: 1, tileSizeV: 1, tileOffsetU: 0, tileOffsetV: 0 } }
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [edit] })
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [{ ...edit, path: [...edit.path, 'tileSizeU'], value: 2 }] })
    assert.equal(objectInfoSchemaOfKind.Brick.parse(state.world.objectOfId[brick.id].info.Brick).material?.tileSizeU, 2)
    state = editorWorldReducer(state, { type: 'edit', description: 'Test edit', edits: [{ ...edit, value: null }] })
    assert.equal(objectInfoSchemaOfKind.Brick.parse(state.world.objectOfId[brick.id].info.Brick).material, null)
    for (const { action } of state.unackedActions) state = editorWorldReducer(state, { type: 'acked', action, seq: 1 })
    state = editorWorldReducer(state, { type: 'refresh', world, seq: 1 })
    assert.equal(state.world, world)
    const incoming = structuredClone(world)
    incoming.objectOfId[brick.id].info = { LocalGroup: {} }
    state = editorWorldReducer(state, { type: 'refresh', world: incoming })
    assert.deepEqual(state.world.objectOfId[brick.id].info, { LocalGroup: {} })
})

test('physics edits update their separate storage maps without touching ObjectInfo or siblings', () => {
    const world = loadWorld()
    const brick = Object.values(world.objectOfId).find(object => object.info.Brick)
    assert.ok(brick)
    const state = editorWorldReducer(createEditorWorldState(world), { type: 'edit', description: 'Test edit', edits: [{ objectId: brick.id, section: 'physicsInfo3DOfId', path: ['location', 'position'], value: [11, 12, 13] }] })
    assert.equal(state.error, null)
    assert.deepEqual(state.world.physicsInfo3DOfId[brick.id].location.position, [11, 12, 13])
    assert.deepEqual(state.world.physicsInfo3DOfId[brick.id].movement, world.physicsInfo3DOfId[brick.id].movement)
    assert.equal(state.world.objectOfId, world.objectOfId)
})

test('search finds collapsed descendants by name or exact ID while preserving the tree state', () => {
    const world = loadWorld()
    const id = world.rootObjectId
    const expanded = { [id]: false }
    assert.deepEqual(getExplorerRows(world, expanded, '').map(row => row.objectId), [id])
    const child = Object.values(world.objectOfId).find(object => object.id !== id)
    assert.ok(child)
    const found = getExplorerRows(world, expanded, String(child.id))
    assert.ok(found.some(row => row.objectId === child.id))
    assert.equal(found[0].objectId, id)
    assert.equal(expanded[id], false)
    assert.deepEqual(getExplorerRows(world, expanded, 'no-object-has-this-name'), [])
    assert.deepEqual(getExplorerRows(world, expanded, '   ').map(row => row.objectId), [id])
    world.childrenOfId[id].push(id, Number.MAX_SAFE_INTEGER)
    assert.equal(getExplorerRows(world, {}, '').filter(row => row.objectId === id).length, 1)
})

test('nullable material fields choose real references and never invent object IDs', () => {
    const world = loadWorld()
    const schema = fieldSchemaOfKind.Brick.properties?.material
    assert.ok(schema && typeof schema !== 'boolean' && schema.anyOf)
    const initial = getInitialFieldValue(schema.anyOf[0], world)
    assert.ok(initial && typeof initial === 'object' && !Array.isArray(initial))
    assert.equal(typeof initial.materialInfoId, 'number')
    const material = world.objectOfId[String(initial.materialInfoId)]
    assert.ok(material.info.Material)
    for (const object of Object.values(world.objectOfId)) if (object.info.Material) delete world.objectOfId[object.id]
    assert.equal(getInitialFieldValue(schema.anyOf[0], world), undefined)
})

test('degree controls apply y first, so a yaw past a quarter turn reads back as written', () => {
    const degrees = quaternionToDegrees(degreesToQuaternion([30, 120, 45]))
    degrees.forEach((angle, axis) => assert.ok(Math.abs(angle - [30, 120, 45][axis]) < 1e-8, `${degrees}`))
})

test('XYZ degree controls round trip quaternion rotations, including both gimbal locks', () => {
    for (const degrees of [[0, 0, 0], [25, 50, 75], [359, 180, 270], [90, 30, 40], [-90, 30, 40], [89.99999, 30, 40], [-720, 18, 450], [1e308, 0, 0]]) {
        const quaternion = degreesToQuaternion(degrees)
        const result = degreesToQuaternion(quaternionToDegrees(quaternion))
        const dot = quaternion.reduce((sum, value, index) => sum + value * result[index], 0)
        assert.ok(Math.abs(Math.abs(dot) - 1) < 1e-6, `${degrees}: ${dot}`)
        assert.ok(Math.abs(Math.hypot(...result) - 1) < 1e-10)
    }
})

test('only property edits and script text are sent to the server', () => {
    const world = loadWorld()
    const script = Object.values(world.objectOfId).find(object => object.info.Script)
    assert.ok(script)
    let state = createEditorWorldState(world)
    const edit: EditorClientDelta = { objectId: script.id, section: 'objectOfId', path: ['name'], value: 'Renamed' }
    state = editorWorldReducer(state, { type: 'edit', description: 'Name on Script', edits: [edit] })
    assert.deepEqual(state.actionsToSend, [{ description: 'Name on Script', deltas: [edit] }], 'A property edit travels unchanged')
    state = editorWorldReducer(state, { type: 'acked', action: state.actionsToSend[0], seq: 1 })
    assert.deepEqual(state.actionsToSend, [], 'An answered action leaves the queue')
    state = editorWorldReducer(state, { type: 'edit-script', objectId: script.id, text: 'const x = 1;' })
    assert.deepEqual(state.actionsToSend, [{ description: `Script ${script.id}`, deltas: [{ objectId: script.id, section: 'scriptRawCodeOfId', path: [], value: 'const x = 1;' }] }])
    state = editorWorldReducer(state, { type: 'rejected', action: state.actionsToSend[0] })
    assert.deepEqual(state.actionsToSend, [])

    // Selection, tabs, and incoming server state are local, so none of them may reach the server as an edit.
    for (const action of [{ type: 'select', objectId: script.id }, { type: 'pick', objectId: null }, { type: 'show-world' }, { type: 'close-script', objectId: script.id }, { type: 'refresh', world }] as const) {
        assert.deepEqual(editorWorldReducer(state, action).actionsToSend, [], action.type)
    }
})
