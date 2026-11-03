import assert from 'node:assert/strict'
import test from 'node:test'
import type { UIMessage } from 'ai'
import { getMessageUpdate } from '../crates/editor_server/node/src/helpers/messageUpdates.ts'
import { applyMessageUpdate } from '../packages/website/app/editor/editorChat.ts'

// A reply as the server's stream gives it after each piece arrives: thinking grows, a tool runs, the list of changed
// objects near the top is overwritten in place, a second tool runs, and the answer is added at the end.
type Part = UIMessage['parts'][number]
const reply = (parts: Part[]): UIMessage => ({ id: 'reply', role: 'assistant', metadata: { startedAtMs: 1_000 }, parts })
const thinking = (text: string): Part => ({ type: 'reasoning', text, state: 'streaming' })
const inspect = (toolCallId: string, isDone: boolean): Part => isDone
    ? { type: 'dynamic-tool', toolName: 'inspect', toolCallId, state: 'output-available', input: {}, output: 'A red car' }
    : { type: 'dynamic-tool', toolName: 'inspect', toolCallId, state: 'input-available', input: {} }
const objectChanges = (names: string[]): Part => ({ type: 'data-object-changes', id: 'object-changes', data: names.map((name, index) => ({ objectId: index + 1, name, kind: 'Brick', change: 'created' })) })
const replySnapshots = [
    reply([]),
    reply([{ type: 'step-start' }, thinking('Planning')]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', false)]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', true), objectChanges(['Track'])]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', true), objectChanges(['Track']), { type: 'step-start' }, inspect('call-2', false)]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', true), objectChanges(['Track', 'Red car']), { type: 'step-start' }, inspect('call-2', true)]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', true), objectChanges(['Track', 'Red car']), { type: 'step-start' }, inspect('call-2', true), { type: 'step-start' }, { type: 'text', text: 'Done', state: 'streaming' }]),
    reply([{ type: 'step-start' }, thinking('Planning the track'), inspect('call-1', true), objectChanges(['Track', 'Red car']), { type: 'step-start' }, inspect('call-2', true), { type: 'step-start' }, { type: 'text', text: 'Done', state: 'streaming' }]),
]
const userMessage: UIMessage = { id: 'question', role: 'user', parts: [{ type: 'text', text: 'Build a race track' }] }

// Replays the snapshots the way chat.ts publishes them and useEditorChat applies them, through a copy as on the wire.
const publishAndApply = (snapshots: UIMessage[], published: UIMessage | undefined, browserMessages: UIMessage[]) => {
    const updates = []
    for (const latest of snapshots) {
        const update = structuredClone(getMessageUpdate(published, latest))
        updates.push(update)
        if (!update) continue
        published = latest
        if (update.type === 'message') {
            const index = browserMessages.findIndex(message => message.id === update.message.id)
            browserMessages = index === -1 ? [...browserMessages, update.message] : browserMessages.with(index, update.message)
        } else browserMessages = applyMessageUpdate(browserMessages, update)
        assert.deepEqual(browserMessages, [userMessage, latest])
    }
    return updates
}

test('A browser that applies each update holds exactly the reply the server has', () => {
    const updates = publishAndApply(replySnapshots, undefined, [userMessage])
    assert.equal(updates[0]?.type, 'message')
    assert.equal(updates.at(-1), undefined, 'a reply that did not change publishes nothing')
})

test('An update carries only the parts that changed, even when one near the top changes in place', () => {
    const updates = publishAndApply(replySnapshots, undefined, [userMessage])
    const changedIndices = updates.map(update => update?.type === 'message-update' ? update.changedParts.map(({ index }) => index) : undefined)
    // The thinking grows, then the first tool's result and the list of changed objects arrive, then the list is overwritten
    // next to the second tool's result: the tool call, the thinking and the step markers before them are not sent again.
    assert.deepEqual(changedIndices, [undefined, [0, 1], [2], [2, 3], [4, 5], [3, 5], [6, 7], undefined])
})

test('A text or reasoning part whose text only grew sends just the added text', () => {
    const answer = (text: string, state: 'streaming' | 'done'): Part => ({ type: 'text', text, state })
    const snapshots = [
        reply([{ type: 'step-start' }, answer('The', 'streaming')]),
        reply([{ type: 'step-start' }, answer('The track is', 'streaming')]),
        reply([{ type: 'step-start' }, answer('The track is ready', 'done')]),
        reply([{ type: 'step-start' }, answer('A different answer', 'done')]),
    ]
    const updates = publishAndApply(snapshots, undefined, [userMessage])
    const sentPieces = updates.map(update => update?.type === 'message-update' ? { changedIndices: update.changedParts.map(({ index }) => index), addedTexts: update.addedTexts } : undefined)
    // A part whose state changed, or whose text no longer starts with the text already sent, is sent whole.
    assert.deepEqual(sentPieces, [
        undefined,
        { changedIndices: [], addedTexts: [{ index: 1, text: ' track is' }] },
        { changedIndices: [1], addedTexts: [] },
        { changedIndices: [1], addedTexts: [] },
    ])
})

test('A browser that connects mid-reply starts from the reply as last published and catches up', () => {
    publishAndApply(replySnapshots.slice(4), replySnapshots[3], [userMessage, replySnapshots[3]])
})
