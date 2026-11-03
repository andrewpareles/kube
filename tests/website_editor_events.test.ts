import assert from 'node:assert/strict'
import test from 'node:test'
import { readEditorEvents } from '../packages/website/app/editor/editorEvents.ts'

// Real streams exercise framing and cancellation without an HTTP server or transport mocks.
test('editor events survive arbitrary byte boundaries and multibyte chat content', async () => {
    const expected = [{ type: 'state', messages: [] }, { type: 'message', text: '世界 🌎' }]
    const bytes = new TextEncoder().encode(expected.map(value => JSON.stringify(value)).join('\n') + '\n')
    const response = new Response(new ReadableStream({ start(controller) { for (const byte of bytes) controller.enqueue(new Uint8Array([byte])); controller.close() } }))
    assert.deepEqual(await Array.fromAsync(readEditorEvents(response)), expected)
})

test('leaving a tool or chat stream releases its reader and cancels further delivery', async () => {
    let didCancel = false
    const response = new Response(new ReadableStream({
        start(controller) { controller.enqueue(new TextEncoder().encode('{"type":"result"}\n')) },
        cancel() { didCancel = true },
    }))
    for await (const event of readEditorEvents(response)) { assert.deepEqual(event, { type: 'result' }); break }
    assert.equal(didCancel, true)
    assert.equal(response.body?.locked, false)
})

test('malformed or truncated events fail rather than being accepted as a completed action', async () => {
    for (const body of ['{', '{"type":', '{invalid}\n']) {
        await assert.rejects(Array.fromAsync(readEditorEvents(new Response(body))), SyntaxError)
    }
    await assert.rejects(Array.fromAsync(readEditorEvents(new Response(null))), /no event stream/)
})
