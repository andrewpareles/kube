import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createEventBroadcast } from '../src/helpers/eventBroadcast.ts'

const read = async (reader: ReadableStreamDefaultReader<Uint8Array>) => {
    const chunk = await reader.read()
    assert.equal(chunk.done, false)
    return JSON.parse(new TextDecoder().decode(chunk.value)) as unknown
}

test('all clients receive the same events, and closing one does not stop the others', async () => {
    const events = createEventBroadcast()
    const creator = new AbortController()
    const second = new AbortController()
    const readers = [creator, second].map((controller) =>
        events
            .subscribe(async () => ({ type: 'state', messages: [] }), controller.signal)
            .getReader(),
    )
    for (const reader of readers)
        assert.deepEqual(await read(reader), { type: 'state', messages: [] })
    const message = {
        type: 'message',
        message: { id: 'shared', role: 'assistant', parts: [{ type: 'text', text: 'Hello 世界' }] },
    }
    events.publish(message)
    for (const reader of readers) assert.deepEqual(await read(reader), message)
    second.abort()
    assert.equal((await readers[1].read()).done, true)
    events.publish({
        type: 'message',
        message: { ...message.message, parts: [{ type: 'text', text: 'Finished' }] },
    })
    assert.deepEqual(await read(readers[0]), {
        type: 'message',
        message: { ...message.message, parts: [{ type: 'text', text: 'Finished' }] },
    })
    await readers[0].cancel()
    assert.equal((await readers[0].read()).done, true)
})

test('events arriving during a snapshot are delivered afterward in order', async () => {
    const events = createEventBroadcast()
    const snapshot = Promise.withResolvers<unknown>()
    const reader = events
        .subscribe(() => snapshot.promise, new AbortController().signal)
        .getReader()
    events.publish({ type: 'message', text: 'first' })
    events.publish({ type: 'message', text: 'final' })
    events.closeSubscribers()
    events.publish({ type: 'message', text: 'next turn' })
    snapshot.resolve({ type: 'state', messages: [] })
    assert.deepEqual(await read(reader), { type: 'state', messages: [] })
    assert.deepEqual(await read(reader), { type: 'message', text: 'first' })
    assert.deepEqual(await read(reader), { type: 'message', text: 'final' })
    assert.equal((await reader.read()).done, true)
})

test('a reconnect gets a fresh snapshot and subsequent events without replaying old chunks', async () => {
    const events = createEventBroadcast()
    events.publish({ type: 'message', text: 'old partial' })
    const reader = events
        .subscribe(
            async () => ({ type: 'state', messages: ['completed reply'] }),
            new AbortController().signal,
        )
        .getReader()
    assert.deepEqual(await read(reader), { type: 'state', messages: ['completed reply'] })
    events.publish({ type: 'message', text: 'next reply' })
    assert.deepEqual(await read(reader), { type: 'message', text: 'next reply' })
    await reader.cancel()
})

test('a slow client is disconnected without slowing an active client or retaining its subscription', async () => {
    const events = createEventBroadcast()
    const slow = new AbortController()
    const slowReader = events.subscribe(async () => ({ type: 'state' }), slow.signal).getReader()
    const activeReader = events
        .subscribe(async () => ({ type: 'state' }), new AbortController().signal)
        .getReader()
    await read(activeReader)
    let isDisconnected = false
    const disconnected = assert.rejects(slowReader.closed, /fell behind/).then(() => {
        isDisconnected = true
    })
    for (let index = 0; !isDisconnected; index++) {
        assert.ok(index < 1000, 'The slow queue must be bounded')
        events.publish({ index })
        assert.deepEqual(await read(activeReader), { index })
    }
    await disconnected
    await assert.rejects(slowReader.read(), /fell behind/)
    events.publish({ text: 'still active' })
    assert.deepEqual(await read(activeReader), { text: 'still active' })
    await activeReader.cancel()
})

test('snapshot failures and aborts release the subscription even while the snapshot is pending', async () => {
    const events = createEventBroadcast()
    const failure = new AbortController()
    const failed = events
        .subscribe(async () => {
            throw new Error('Cannot read history')
        }, failure.signal)
        .getReader()
    await assert.rejects(failed.read(), /Cannot read history/)
    await assert.rejects(failed.closed, /Cannot read history/)
    const pending = Promise.withResolvers<unknown>()
    const abort = new AbortController()
    const reader = events.subscribe(() => pending.promise, abort.signal).getReader()
    events.closeSubscribers()
    abort.abort()
    pending.resolve({ type: 'state' })
    assert.equal((await reader.read()).done, true)
    events.publish({ type: 'message' })
})

test('turn completion delivers the final message to all clients and permits fresh subscriptions', async () => {
    const events = createEventBroadcast()
    const readers = [new AbortController(), new AbortController()].map((controller) =>
        events
            .subscribe(async () => ({ type: 'state', messages: [] }), controller.signal)
            .getReader(),
    )
    for (const reader of readers) await read(reader)
    events.publish({ type: 'message', text: 'Final answer' })
    events.closeSubscribers()
    events.closeSubscribers()
    events.publish({ type: 'message', text: 'Later turn' })
    for (const reader of readers) {
        assert.deepEqual(await read(reader), { type: 'message', text: 'Final answer' })
        assert.equal((await reader.read()).done, true)
    }
    const next = events
        .subscribe(
            async () => ({ type: 'state', messages: ['Final answer', 'Later turn'] }),
            new AbortController().signal,
        )
        .getReader()
    assert.deepEqual(await read(next), { type: 'state', messages: ['Final answer', 'Later turn'] })
    events.publish({ type: 'message', text: 'New update' })
    assert.deepEqual(await read(next), { type: 'message', text: 'New update' })
    await next.cancel()
})

test('closing a full stream drains queued messages without treating completion as a slow client', async () => {
    const events = createEventBroadcast()
    const controller = new AbortController()
    const reader = events.subscribe(async () => ({ type: 'state' }), controller.signal).getReader()
    await read(reader)
    for (let index = 0; index < 640; index++) events.publish({ index })
    events.closeSubscribers()
    for (let index = 0; index < 640; index++) assert.deepEqual(await read(reader), { index })
    assert.equal((await reader.read()).done, true)
    controller.abort()
})
