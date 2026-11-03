import assert from 'node:assert/strict'
import { appendFile, readFile, writeFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { applyBrowserEditorDelta, browserEditorDeltaSchema, createEditorClientActionSender, clientSendChangeIntervalMs, editorStateSchema, type EditorClientDelta } from '../packages/website/app/editor/editorSync.ts'
import { repoPath } from './testPaths.ts'
import { withIsPushable } from './authoredWorld.ts'

const readState = async () => editorStateSchema.parse({
    protocol_version: 0,
    seq: 0,
    world: withIsPushable(JSON.parse(await readFile(repoPath('game-35-latest.kube'), 'utf8'))),
    editor_status: { is_playtest_running: false, is_chat_running: false, history_of_session: {} },
})

test('every delta carries status without rebuilding an unchanged world', async () => {
    const initial = await readState()
    const editor_status = { is_playtest_running: true, is_chat_running: true, history_of_session: { someone: { can_undo: true, can_redo: false } } }
    const running = applyBrowserEditorDelta(initial, browserEditorDeltaSchema.parse({ seq: 1, world: null, editor_status }))
    assert.equal(running.world, initial.world, 'Status changes must not rebuild the renderer or clear previews')
    assert.deepEqual(running.editor_status, editor_status)
    assert.equal(applyBrowserEditorDelta(running, { seq: 2, world: null, editor_status }).world, initial.world)
    assert.equal(browserEditorDeltaSchema.safeParse({ seq: 3, world: null, editor_status: null }).success, false)
    const world = structuredClone(initial.world)
    world.objectOfId[world.rootObjectId].name = 'Updated by server'
    const updated = applyBrowserEditorDelta(running, { seq: 4, world, editor_status })
    assert.equal(updated.world, world)
    assert.equal(updated.seq, 4, 'Every batch moves the browser to its number')
    assert.deepEqual(updated.editor_status, running.editor_status)
    assert.equal(initial.world.objectOfId[world.rootObjectId].name, running.world.objectOfId[world.rootObjectId].name)
    // WASM checks protocol compatibility; TypeScript only validates the snapshot's shape.
    for (const protocol_version of [initial.protocol_version, initial.protocol_version + 1]) assert.equal(editorStateSchema.safeParse({ ...initial, protocol_version }).success, true)
    for (const protocol_version of [undefined, '1']) assert.equal(editorStateSchema.safeParse({ ...initial, protocol_version }).success, false)
    assert.equal(editorStateSchema.safeParse({ ...initial, editor_status: undefined }).success, false)
})

test('rapid edits to one field collapse to its newest value while other fields still arrive in order', async t => {
    // A real file is the sender's destination; no HTTP server or transport mocks are used.
    const directory = await mkdtemp(path.join(tmpdir(), 'kube-editor-throttle-'))
    t.after(() => rm(directory, { recursive: true, force: true }))
    const deliveryPath = path.join(directory, 'delivered.ndjson')
    const sender = createEditorClientActionSender(async action => { await appendFile(deliveryPath, `${JSON.stringify({ ...action.deltas[0], sentAt: Date.now() })}\n`) })
    const position: EditorClientDelta = { objectId: 1, section: 'physicsInfo3DOfId', path: ['location', 'position'], value: 0 }
    const name: EditorClientDelta = { objectId: 1, section: 'objectOfId', path: ['name'], value: 'Typed' }

    // Every keystroke queues an edit for the same field, so only the newest may survive the wait.
    for (const value of [1, 2, 3, 4, 5]) sender.send({ description: 'Position on Ball', deltas: [{ ...position, value }] })
    sender.send({ description: 'Name on Ball', deltas: [name] })
    sender.send({ description: 'Name on Ball', deltas: [{ ...name, value: 'Retyped' }] })
    await sender.waitForPendingEditAcks()

    const delivered: (EditorClientDelta & { sentAt: number })[] = (await readFile(deliveryPath, 'utf8')).trim().split('\n').map(line => JSON.parse(line))
    assert.deepEqual(delivered.map(edit => edit.value), [5, 'Retyped'], 'Each field sends once, carrying the value typed last')
    assert.ok(delivered[1].sentAt - delivered[0].sentAt >= clientSendChangeIntervalMs - 10, 'Sends stay throttled apart')
})

test('a failed edit reports its error without blocking later edits, then the sender recovers', async t => {
    const directory = await mkdtemp(path.join(tmpdir(), 'kube-editor-failed-edit-'))
    t.after(() => rm(directory, { recursive: true, force: true }))
    const deliveryPath = path.join(directory, 'delivered.json')
    const sender = createEditorClientActionSender(async action => {
        const [delta] = action.deltas
        await writeFile(delta.section === 'scriptRawCodeOfId' ? path.join(directory, 'missing', 'rejected.json') : deliveryPath, JSON.stringify(delta))
    })
    const delta: EditorClientDelta = { objectId: 1, section: 'objectOfId', path: ['name'], value: 'first' }
    const rejected = sender.send({ description: 'Script 1', deltas: [{ ...delta, section: 'scriptRawCodeOfId', path: [], value: 'const rejected = 1;' }] })
    const accepted = sender.send({ description: 'Name on Ball', deltas: [delta] })
    await assert.rejects(rejected, { code: 'ENOENT' })
    await accepted
    assert.equal(JSON.parse(await readFile(deliveryPath, 'utf8')).value, 'first', 'A rejected edit must not discard the edits queued behind it')

    // Save waits for everything still queued, and a later edit starts a fresh acknowledgement.
    sender.send({ description: 'Name on Ball', deltas: [{ ...delta, value: 'saved snapshot' }] })
    await sender.waitForPendingEditAcks()
    assert.equal(JSON.parse(await readFile(deliveryPath, 'utf8')).value, 'saved snapshot')
})

test('an edit to fields shared with a queued edit still reaches the server after it', async () => {
    const sent: unknown[][] = []
    let releaseFirst!: () => void
    const firstSent = new Promise<void>(resolve => { releaseFirst = resolve })
    const sender = createEditorClientActionSender(async action => {
        if (sent.length === 0) await firstSent
        sent.push(action.deltas.map(delta => delta.value))
        return sent.length
    })
    const x = (objectId: number, value: number): EditorClientDelta => ({ objectId, section: 'physicsInfo3DOfId', path: ['location', 'position'], value })
    const inFlight = sender.send({ description: 'Start', deltas: [x(9, 0)] })
    await new Promise(resolve => setTimeout(resolve, clientSendChangeIntervalMs + 20))
    sender.send({ description: 'Ball', deltas: [x(1, 6)] })
    sender.send({ description: 'Ball and Cube', deltas: [x(1, 7), x(2, 7)] })
    const last = sender.send({ description: 'Ball', deltas: [x(1, 8)] })
    releaseFirst()
    await Promise.all([inFlight, last])
    assert.deepEqual(sent.at(-1), [8], 'The newest edit to Ball is applied last')
})

test('an edit replaced while it waited is acknowledged together with the edit that replaced it', async () => {
    const sent: unknown[] = []
    const sender = createEditorClientActionSender(async action => { sent.push(action.deltas[0].value); return 7 })
    const name: EditorClientDelta = { objectId: 1, section: 'objectOfId', path: ['name'], value: 'Ty' }
    const first = sender.send({ description: 'Name on Ball', deltas: [name] })
    const second = sender.send({ description: 'Name on Ball', deltas: [{ ...name, value: 'Typed' }] })
    assert.deepEqual(await Promise.all([first, second]), [7, 7], 'Both answer with the batch that includes the newest value')
    assert.deepEqual(sent, ['Typed'])
})
