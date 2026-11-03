import { z } from 'zod'
import { getObjectKind, objectInfoSchemaOfKind, physicsInfo3DSchema, physicsToggles3DSchema, primitiveSchemaOfKind, storedWorldSchema, type EditorValue, type StoredWorld } from './editorObjectSchema.ts'

import type { EditorClientAction, EditorClientDelta } from './editorSync.ts'

// React keeps an immediate property preview and local script drafts. The preview is the server's
// latest world with this tab's property edits that world does not include yet put back on top: a
// server world can be older than what was just typed, and without them it would take keystrokes back.
// An edit stays on top until a world batch numbered at or after the one the server said includes it.
type UnackedAction = { action: EditorClientAction, ackedSeq: number | null }
export type EditorWorldState = { world: StoredWorld, serverWorld: StoredWorld, serverSeq: number, selectedObjectIds: number[], pendingSelectedObjectIds: number[], error: string | null, openScriptIds: number[], activeScriptId: number | null, scriptDraftOfId: { [id: number]: string | undefined }, actionsToSend: EditorClientAction[], unackedActions: UnackedAction[] }
export type EditorWorldAction = { type: 'select', objectId: number | null } | { type: 'select-objects', objectIds: number[] } | { type: 'pick', objectId: number | null } | { type: 'toggle-select', objectId: number | null } | { type: 'toggle-pick', objectId: number | null } | { type: 'edit', description: string, edits: EditorClientDelta[] } | { type: 'acked', action: EditorClientAction, seq: number } | { type: 'rejected', action: EditorClientAction } | { type: 'refresh', world: StoredWorld, seq: number } | { type: 'show-world' } | { type: 'close-script', objectId: number } | { type: 'edit-script', objectId: number, text: string }
export const createEditorWorldState = (world: StoredWorld, seq = 0): EditorWorldState => ({ world, serverWorld: world, serverSeq: seq, selectedObjectIds: [], pendingSelectedObjectIds: [], error: null, openScriptIds: [], activeScriptId: null, scriptDraftOfId: {}, actionsToSend: [], unackedActions: [] })

// Script drafts stay separate from the rendered world so typing does not rebuild the scene.
const isEditableScript = (world: StoredWorld, objectId: number) => world.objectOfId[objectId] && getObjectKind(world.objectOfId[objectId]) === 'Script' && Object.hasOwn(world.scriptRawCodeOfId, objectId)

// The editor server stores names this way (normalizedObjectName in world.rs), so the preview shows what it keeps.
const normalizeObjectName = (name: string) => name === '' ? '-' : [...name].slice(0, 64).map(character => /^[A-Za-z0-9_-]$/.test(character) ? character : '-').join('')

// Actions are sent and answered in order, so an answered one and everything queued before it has left.
const withoutSentThrough = (actions: EditorClientAction[], answered: EditorClientAction) => {
    const index = actions.indexOf(answered)
    return index === -1 ? actions : actions.slice(index + 1)
}

// An edit that no longer applies, because its object or property is gone, is dropped from the preview.
const withUnackedActions = (serverWorld: StoredWorld, unackedActions: UnackedAction[]) => {
    let world = serverWorld
    const scriptDraftOfId: { [id: number]: string | undefined } = {}
    const kept = unackedActions.filter(({ action }) => {
        try {
            const edited = action.deltas.reduce(applyEditorClientDelta, world)
            if (action.deltas.some(delta => delta.section !== 'scriptRawCodeOfId')) world = edited
            else for (const { objectId } of action.deltas) scriptDraftOfId[objectId] = edited.scriptRawCodeOfId[objectId]
            return true
        }
        catch { return false }
    })
    return { world, unackedActions: kept, scriptDraftOfId: Object.fromEntries(Object.entries(scriptDraftOfId).filter(([id, text]) => text !== world.scriptRawCodeOfId[Number(id)])) }
}

// Replace only an existing field, copying its ancestors and preserving all siblings.
const replaceField = (value: EditorValue, path: string[], replacement: EditorValue): EditorValue => {
    if (!path.length) return replacement
    const [field, ...rest] = path
    if (value === null || typeof value !== 'object' || Array.isArray(value) || !Object.hasOwn(value, field)) throw new Error('This property no longer exists.')
    return { ...value, [field]: replaceField(value[field], rest, replacement) }
}

const applyEditorClientDelta = (world: StoredWorld, edit: EditorClientDelta): StoredWorld => {
    const { objectId, section, path, value } = edit
    const object = world.objectOfId[objectId]
    if (!object) throw new Error('This object no longer exists.')
    const kind = getObjectKind(object)
    if (section === 'scriptRawCodeOfId') {
        if (kind !== 'Script' || path.length || typeof value !== 'string') throw new Error('Invalid script edit.')
        return { ...world, scriptRawCodeOfId: { ...world.scriptRawCodeOfId, [objectId]: value } }
    }
    if (section === 'objectOfId') {
        if (path.length === 1 && path[0] === 'name' && typeof value === 'string') return { ...world, objectOfId: { ...world.objectOfId, [objectId]: { ...object, name: normalizeObjectName(value) } } }
        if (path[0] !== 'info' || path[1] !== kind || path.length < 3) throw new Error('This property cannot be edited.')
        const info = replaceField(object.info[kind], path.slice(2), value)
        objectInfoSchemaOfKind[kind].parse(info)
        return { ...world, objectOfId: { ...world.objectOfId, [objectId]: { ...object, info: { [kind]: info } } } }
    }
    const current = world[section][objectId]
    if (!current) throw new Error('This property no longer exists.')
    const updated = replaceField(current, path, value)
    if (section === 'physicsInfo3DOfId') return { ...world, physicsInfo3DOfId: { ...world.physicsInfo3DOfId, [objectId]: physicsInfo3DSchema.parse(updated) } }
    if (section === 'physicsToggles3DOfId') return { ...world, physicsToggles3DOfId: { ...world.physicsToggles3DOfId, [objectId]: physicsToggles3DSchema.parse(updated) } }
    const primitive = storedWorldSchema.shape.primitiveSettingsOfId.valueType.parse(updated)
    primitiveSchemaOfKind[kind].parse(primitive[kind])
    return { ...world, primitiveSettingsOfId: { ...world.primitiveSettingsOfId, [objectId]: primitive } }
}

export const editorWorldReducer = (state: EditorWorldState, action: EditorWorldAction): EditorWorldState => {
    if (action.type === 'show-world') return { ...state, activeScriptId: null }
    if (action.type === 'close-script') {
        const index = state.openScriptIds.indexOf(action.objectId)
        if (index === -1) return state
        const openScriptIds = state.openScriptIds.filter(id => id !== action.objectId)
        const activeScriptId = state.activeScriptId === action.objectId ? openScriptIds[Math.min(index, openScriptIds.length - 1)] ?? null : state.activeScriptId
        return { ...state, openScriptIds, activeScriptId, selectedObjectIds: activeScriptId === null ? state.selectedObjectIds : [activeScriptId] }
    }
    if (action.type === 'edit-script') {
        if (!isEditableScript(state.world, action.objectId)) return state
        const current = state.scriptDraftOfId[action.objectId] ?? state.world.scriptRawCodeOfId[action.objectId]
        if (current === action.text) return state
        const scriptDraftOfId = { ...state.scriptDraftOfId, [action.objectId]: action.text }
        if (action.text === state.world.scriptRawCodeOfId[action.objectId]) delete scriptDraftOfId[action.objectId]
        // The draft is the preview, so only the server's copy is queued; the world changes when it answers.
        const sent: EditorClientAction = { description: `Script ${action.objectId}`, deltas: [{ objectId: action.objectId, section: 'scriptRawCodeOfId', path: [], value: action.text }] }
        return { ...state, scriptDraftOfId, actionsToSend: [...state.actionsToSend, sent], unackedActions: [...state.unackedActions, { action: sent, ackedSeq: null }] }
    }
    if (action.type === 'acked' && action.seq > state.serverSeq) return { ...state, actionsToSend: withoutSentThrough(state.actionsToSend, action.action), unackedActions: state.unackedActions.map(unacked => unacked.action === action.action ? { ...unacked, ackedSeq: action.seq } : unacked) }
    // Rejected, or already in the server's latest world: the preview is rebuilt without it.
    if (action.type === 'acked' || action.type === 'rejected') return { ...state, actionsToSend: withoutSentThrough(state.actionsToSend, action.action), ...withUnackedActions(state.serverWorld, state.unackedActions.filter(unacked => unacked.action !== action.action)) }
    // An undo can bring back objects whose return reaches this browser a moment after its answer, so
    // those are selected when the world that holds them arrives.
    if (action.type === 'select-objects') return { ...state, error: null, selectedObjectIds: action.objectIds.filter(id => state.world.objectOfId[id]), pendingSelectedObjectIds: action.objectIds.filter(id => !state.world.objectOfId[id]) }
    if (action.type === 'select' || action.type === 'pick' || action.type === 'toggle-select' || action.type === 'toggle-pick') {
        const isMultiSelect = action.type === 'toggle-select' || action.type === 'toggle-pick'
        let selectedObjectId = action.objectId !== null && state.world.objectOfId[action.objectId] ? action.objectId : null
        if ((action.type === 'pick' || action.type === 'toggle-pick') && selectedObjectId !== null && !(isMultiSelect && state.selectedObjectIds.includes(selectedObjectId))) {
            // Viewport hits enter one rigid group at a time. Explorer and reference selections stay exact.
            const parentOfId: { [id: number]: number } = {}
            for (const [parent, children] of Object.entries(state.world.childrenOfId)) for (const child of children) parentOfId[child] = Number(parent)
            const selectedAncestors = new Set<number>()
            for (const selectedId of state.selectedObjectIds) for (let id: number | undefined = selectedId; id !== undefined && !selectedAncestors.has(id); id = parentOfId[id]) selectedAncestors.add(id)

            // Groups containing the current selection are already open for picking their children.
            const visited = new Set([selectedObjectId])
            for (let parent = parentOfId[selectedObjectId]; parent !== undefined && !visited.has(parent); parent = parentOfId[parent]) {
                visited.add(parent)
                const object = state.world.objectOfId[parent]
                if (!object || getObjectKind(object) !== 'RigidGroup') continue
                if (selectedAncestors.has(parent)) {
                    if (isMultiSelect && state.selectedObjectIds.includes(parent)) selectedObjectId = parent
                    break
                }
                selectedObjectId = parent
            }
        }
        // Modifier clicks toggle exact selections; empty modifier clicks preserve the current selection.
        let selectedObjectIds = selectedObjectId === null ? [] : [selectedObjectId]
        if (isMultiSelect) {
            selectedObjectIds = selectedObjectId === null ? state.selectedObjectIds : state.selectedObjectIds.includes(selectedObjectId) ? state.selectedObjectIds.filter(id => id !== selectedObjectId) : [...state.selectedObjectIds, selectedObjectId]
        }
        const activeScriptId = !isMultiSelect && selectedObjectId !== null && isEditableScript(state.world, selectedObjectId) ? selectedObjectId : null
        const openScriptIds = activeScriptId !== null && !state.openScriptIds.includes(activeScriptId) ? [...state.openScriptIds, activeScriptId] : state.openScriptIds
        return { ...state, error: null, selectedObjectIds, pendingSelectedObjectIds: [], openScriptIds, activeScriptId }
    }
    // One user action applies its deltas to the preview and queues those same deltas for the editor
    // server. Sending what this reducer applied keeps a burst of edits in the order they happened.
    if (action.type === 'edit') {
        if (!action.edits.length) return state
        let world = state.world
        try { for (const delta of action.edits) world = applyEditorClientDelta(world, delta) }
        catch (error) { return { ...state, error: error instanceof z.ZodError ? error.issues[0].message : error instanceof Error ? error.message : String(error) } }
        const sent = { description: action.description, deltas: action.edits }
        return { ...state, world, error: null, actionsToSend: [...state.actionsToSend, sent], unackedActions: [...state.unackedActions, { action: sent, ackedSeq: null }] }
    }
    const { world, unackedActions, scriptDraftOfId } = withUnackedActions(action.world, state.unackedActions.filter(unacked => unacked.ackedSeq === null || unacked.ackedSeq > action.seq))
    const openScriptIds = state.openScriptIds.filter(id => isEditableScript(world, id))
    const arrivedObjectIds = state.pendingSelectedObjectIds.filter(id => world.objectOfId[id])
    return { world, serverWorld: action.world, serverSeq: action.seq, selectedObjectIds: [...state.selectedObjectIds.filter(id => world.objectOfId[id]), ...arrivedObjectIds], pendingSelectedObjectIds: state.pendingSelectedObjectIds.filter(id => !world.objectOfId[id]), error: null, openScriptIds, activeScriptId: state.activeScriptId !== null && openScriptIds.includes(state.activeScriptId) ? state.activeScriptId : null, scriptDraftOfId, actionsToSend: state.actionsToSend, unackedActions }
}

// Searching includes matching descendants and their ancestors without changing expansion state.
export const getExplorerRows = (world: StoredWorld, isExpandedOfId: { [id: number]: boolean }, search: string) => {
    const rows: { objectId: number, parentId: number | null, depth: number, hasChildren: boolean, isExpanded: boolean }[] = []
    const visited = new Set<number>()
    const query = search.trim().toLowerCase()
    const visit = (objectId: number, parentId: number | null, depth: number) => {
        const object = world.objectOfId[objectId]
        if (!object || visited.has(objectId)) return false
        visited.add(objectId)
        const children = (world.childrenOfId[objectId] ?? []).filter(id => world.objectOfId[id]).toSorted((left, right) => world.objectOfId[left].name.localeCompare(world.objectOfId[right].name) || left - right)
        const isExpanded = query !== '' || (isExpandedOfId[objectId] ?? depth <= 1)
        const index = rows.length
        rows.push({ objectId, parentId, depth, hasChildren: children.length > 0, isExpanded })
        let didMatchChild = false
        if (isExpanded) for (const child of children) didMatchChild = visit(child, objectId, depth + 1) || didMatchChild
        if (query && !object.name.toLowerCase().includes(query) && String(objectId) !== query && !didMatchChild) { rows.splice(index); return false }
        return true
    }
    visit(world.rootObjectId, null, 0)
    return rows
}
