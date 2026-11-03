import { mock, test } from 'node:test'
import assert from 'node:assert/strict'
import { once } from 'node:events'
import { createServer } from 'node:http'
import { createAnthropic } from '@ai-sdk/anthropic'
import {
    convertToModelMessages,
    createUIMessageStream,
    dynamicTool,
    jsonSchema,
    readUIMessageStream,
    type ModelMessage,
    type UIMessage,
    type UIMessageChunk,
    type UIMessageStreamOutcome,
} from 'ai'
import {
    MockLanguageModelV4,
    convertArrayToReadableStream,
    convertReadableStreamToArray,
} from 'ai/test'
import { z } from 'zod'

Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'this-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: '8080',
    EDITOR_CHAT_PORT: '8081',
    SITE_URL: 'https://website.example',
    SUPABASE_URL: 'https://supabase.invalid',
    SUPABASE_SECRET_KEY: 'test-only',
    S3_ACCESS_KEY_ID: 'test-only',
    S3_SECRET_ACCESS_KEY: 'test-only',
    AWS_REGION: 'us-east-1',
    AWS_BEARER_TOKEN_BEDROCK: 'test-only',
    AZURE_OPENAI_API_KEY: 'test-only',
})
const { convertDataPart } = await import('../src/helpers/chat.ts')
const { isCompactionNeeded, retryWithBackoff, streamCompactingReply, toModelInput } =
    await import('../src/helpers/compaction.ts')
const { getModelRoute } = await import('../src/helpers/model.ts')
mock.method(console, 'error', () => undefined)
const summaryText = (summary: string) =>
    `<conversation_summary>\n${summary}\n</conversation_summary>`
const history: UIMessage[] = [
    { id: 'u1', role: 'user', parts: [{ type: 'text', text: 'Build a race track' }] },
    {
        id: 'a1',
        role: 'assistant',
        parts: [
            { type: 'text', text: 'Old work' },
            { type: 'data-compaction', id: 'c1', data: { summary: 'First summary' } },
            { type: 'text', text: 'More old work' },
        ],
    },
    { id: 'u2', role: 'user', parts: [{ type: 'text', text: 'Add a red car' }] },
    {
        id: 'a2',
        role: 'assistant',
        parts: [
            { type: 'step-start' },
            { type: 'text', text: 'Before the summary' },
            { type: 'data-compaction', id: 'c2', data: { summary: 'Latest summary' } },
            { type: 'step-start' },
            { type: 'text', text: 'After the summary' },
            { type: 'data-compaction', id: 'c3', data: {} },
            { type: 'data-object-changes', id: 'object-changes', data: [] },
        ],
    },
    {
        id: 'u3',
        role: 'user',
        parts: [
            { type: 'text', text: 'Make it faster' },
            {
                type: 'data-selected-objects',
                data: [{ objectId: 7, name: 'Red Car', kind: 'RigidGroup' }],
            },
        ],
    },
]

test('Without a summary the model reads the whole history', async () => {
    const messages = [history[0]!, history[2]!, history[4]!]
    assert.deepEqual(
        await toModelInput(messages, { convertDataPart }),
        await convertToModelMessages(messages, { convertDataPart }),
    )
})

test('The model starts from the latest summary, the rest of its reply, and every later message', async () => {
    assert.deepEqual(await toModelInput(history, { convertDataPart }), [
        { role: 'user', content: summaryText('Latest summary') },
        { role: 'assistant', content: [{ type: 'text', text: 'After the summary' }] },
        {
            role: 'user',
            content: [
                { type: 'text', text: 'Make it faster' },
                {
                    type: 'text',
                    text: '<selected_objects>\nThe user had these objects selected in the editor when sending this message:\n{"object_id":7,"name":"Red Car","kind":"RigidGroup"}\n</selected_objects>',
                },
            ],
        },
    ])
})

test('A compaction that never got its summary is ignored', async () => {
    const messages: UIMessage[] = [
        history[0]!,
        {
            id: 'a1',
            role: 'assistant',
            parts: [
                { type: 'text', text: 'Working' },
                { type: 'data-compaction', id: 'c1', data: {} },
            ],
        },
    ]
    assert.deepEqual(await toModelInput(messages, { convertDataPart }), [
        { role: 'user', content: [{ type: 'text', text: 'Build a race track' }] },
        { role: 'assistant', content: [{ type: 'text', text: 'Working' }] },
    ])
    assert.equal(
        convertDataPart({ type: 'data-compaction', data: { summary: 'Latest summary' } }),
        undefined,
    )
})

test('Reasoning shown in the chat is never sent back to the model', async () => {
    const messages: UIMessage[] = [
        history[0]!,
        {
            id: 'a1',
            role: 'assistant',
            parts: [
                { type: 'step-start' },
                {
                    type: 'reasoning',
                    text: 'Planning the track',
                    state: 'done',
                    providerMetadata: { anthropic: { signature: 'signature' } },
                },
                { type: 'text', text: 'Done' },
            ],
        },
    ]
    assert.deepEqual(await toModelInput(messages, { convertDataPart }), [
        { role: 'user', content: [{ type: 'text', text: 'Build a race track' }] },
        { role: 'assistant', content: [{ type: 'text', text: 'Done' }] },
    ])
})

test('Reasoning around a summary, and a reply stopped while thinking, leave nothing for the model', async () => {
    const messages: UIMessage[] = [
        history[0]!,
        {
            id: 'a1',
            role: 'assistant',
            parts: [
                { type: 'step-start' },
                { type: 'reasoning', text: 'Old plan', state: 'done' },
                { type: 'text', text: 'Old work' },
                { type: 'data-compaction', id: 'c1', data: { summary: 'Latest summary' } },
                { type: 'step-start' },
                { type: 'reasoning', text: 'New plan', state: 'done' },
                { type: 'text', text: 'After the summary' },
            ],
        },
        { id: 'u2', role: 'user', parts: [{ type: 'text', text: 'Make it faster' }] },
        {
            id: 'a2',
            role: 'assistant',
            parts: [
                { type: 'step-start' },
                { type: 'reasoning', text: 'Only thinking', state: 'streaming' },
            ],
        },
        { id: 'u3', role: 'user', parts: [{ type: 'text', text: 'Hello?' }] },
    ]
    assert.deepEqual(await toModelInput(messages, { convertDataPart }), [
        { role: 'user', content: summaryText('Latest summary') },
        { role: 'assistant', content: [{ type: 'text', text: 'After the summary' }] },
        { role: 'user', content: [{ type: 'text', text: 'Make it faster' }] },
        { role: 'user', content: [{ type: 'text', text: 'Hello?' }] },
    ])
})

test('Compaction triggers at the limit and not below', () => {
    const usage = (inputTokens: number | undefined) =>
        ({ inputTokens }) as Parameters<typeof isCompactionNeeded>[0]
    assert.equal(isCompactionNeeded(usage(949_999), 950_000), false)
    assert.equal(isCompactionNeeded(usage(950_000), 950_000), true)
    assert.equal(isCompactionNeeded(usage(undefined), 950_000), false)
})

test('A failed summary is retried until it succeeds', async () => {
    let attempts = 0
    const summary = await retryWithBackoff(
        async () => {
            attempts++
            if (attempts < 3) throw new Error('Bedrock is down')
            return 'Summary'
        },
        new AbortController().signal,
        { delaysMs: [1, 2], giveUpAfterMs: 1000 },
    )
    assert.equal(summary, 'Summary')
    assert.equal(attempts, 3)
})

test('Stopping the reply stops the retries at once', async () => {
    const controller = new AbortController()
    let attempts = 0
    const startedAt = Date.now()
    await assert.rejects(
        retryWithBackoff(
            async () => {
                attempts++
                setTimeout(() => controller.abort(), 10)
                throw new Error('Bedrock is down')
            },
            controller.signal,
            { delaysMs: [60_000], giveUpAfterMs: 120_000 },
        ),
        { name: 'AbortError' },
    )
    assert.equal(attempts, 1)
    assert.ok(Date.now() - startedAt < 1000)
    await assert.rejects(
        retryWithBackoff(async () => {
            attempts++
            throw new Error('Aborted mid-call')
        }, controller.signal),
        { name: 'AbortError' },
    )
    assert.equal(attempts, 2)
})

test('A summary that keeps failing gives up only after the bound', async () => {
    let attempts = 0
    const startedAt = Date.now()
    await assert.rejects(
        retryWithBackoff(
            async () => {
                attempts++
                throw new Error(`Bedrock is down ${attempts}`)
            },
            new AbortController().signal,
            { delaysMs: [5], giveUpAfterMs: 60 },
        ),
        (error: Error) => error.message === `Bedrock is down ${attempts}`,
    )
    assert.ok(attempts > 3, `only ${attempts} attempts`)
    assert.ok(Date.now() - startedAt >= 50)
})

type ModelCall = (options: { abortSignal?: AbortSignal }) => unknown
const usage = (inputTokens: number) => ({
    inputTokens: { total: inputTokens, noCache: 0, cacheRead: inputTokens, cacheWrite: 0 },
    outputTokens: { total: 10, text: 10, reasoning: 0 },
})
const textCall =
    (text: string, inputTokens: number): ModelCall =>
    () => ({
        stream: convertArrayToReadableStream([
            { type: 'stream-start', warnings: [] },
            { type: 'text-start', id: text },
            { type: 'text-delta', id: text, delta: text },
            { type: 'text-end', id: text },
            {
                type: 'finish',
                finishReason: { unified: 'stop', raw: 'stop' },
                usage: usage(inputTokens),
            },
        ]),
    })
const toolCall =
    (toolCallId: string, inputTokens: number): ModelCall =>
    () => ({
        stream: convertArrayToReadableStream([
            { type: 'stream-start', warnings: [] },
            { type: 'tool-call', toolCallId, toolName: 'inspect', input: '{}' },
            {
                type: 'finish',
                finishReason: { unified: 'tool-calls', raw: 'tool_calls' },
                usage: usage(inputTokens),
            },
        ]),
    })
const reasoningCall: ModelCall = () => ({
    stream: convertArrayToReadableStream([
        { type: 'stream-start', warnings: [] },
        { type: 'reasoning-start', id: 'thinking' },
        { type: 'reasoning-delta', id: 'thinking', delta: 'Planning the track' },
        {
            type: 'reasoning-delta',
            id: 'thinking',
            delta: '',
            providerMetadata: { anthropic: { signature: 'signature' } },
        },
        {
            type: 'reasoning-end',
            id: 'thinking',
            providerMetadata: { anthropic: { signature: 'signature' } },
        },
        { type: 'text-start', id: 'Done' },
        { type: 'text-delta', id: 'Done', delta: 'Done' },
        { type: 'text-end', id: 'Done' },
        { type: 'finish', finishReason: { unified: 'stop', raw: 'stop' }, usage: usage(1_000) },
    ]),
})
// GPT's reasoning has no readable text, only an encrypted copy the model can read back.
const gptReasoningMetadata = { openai: { itemId: 'rs_1', reasoningEncryptedContent: 'encrypted' } }
const gptReasoningCall: ModelCall = () => ({
    stream: convertArrayToReadableStream([
        { type: 'stream-start', warnings: [] },
        { type: 'reasoning-start', id: 'rs_1', providerMetadata: gptReasoningMetadata },
        { type: 'reasoning-delta', id: 'rs_1', delta: '', providerMetadata: gptReasoningMetadata },
        { type: 'reasoning-end', id: 'rs_1', providerMetadata: gptReasoningMetadata },
        { type: 'text-start', id: 'Done' },
        { type: 'text-delta', id: 'Done', delta: 'Done' },
        { type: 'text-end', id: 'Done' },
        { type: 'finish', finishReason: { unified: 'stop', raw: 'stop' }, usage: usage(1_000) },
    ]),
})
const brokenStreamCall: ModelCall = () => ({
    stream: convertArrayToReadableStream([
        { type: 'stream-start', warnings: [] },
        { type: 'text-start', id: 'partial' },
        { type: 'text-delta', id: 'partial', delta: 'Half a' },
        { type: 'error', error: { type: 'api_error', message: 'Internal server error' } },
    ]),
})
const failingCall: ModelCall = () => {
    throw new Error('Bedrock is down')
}

const raceTrackRequest: ModelMessage[] = [{ role: 'user', content: 'Build a race track' }]
const runReply = async (
    calls: ModelCall[],
    {
        controller = new AbortController(),
        giveUpAfterMs = 5000,
        shouldStop = async () => false,
        messages = raceTrackRequest,
    } = {},
) => {
    const model = new MockLanguageModelV4({
        doStream: async (options) =>
            (calls.shift() ?? failingCall)(options) as Awaited<
                ReturnType<MockLanguageModelV4['doStream']>
            >,
    })
    let toolRuns = 0
    const tools = {
        inspect: dynamicTool({
            description: 'Inspect the world',
            inputSchema: jsonSchema({ type: 'object', properties: {} }),
            execute: async () => {
                toolRuns++
                return 'A red car'
            },
        }),
    }
    let outcome: UIMessageStreamOutcome | undefined
    const stream = createUIMessageStream({
        onError: (error) => (error instanceof Error ? error.message : String(error)),
        onEnd: (event) => {
            outcome = event.outcome
        },
        execute: ({ writer }) =>
            streamCompactingReply({
                writer,
                call: {
                    model,
                    providerOptions: { openai: { store: true } },
                    system: 'System',
                    tools,
                },
                compactionInputTokens: 220_000,
                signal: controller.signal,
                messages,
                shouldStop,
                // The reply carries its start time the way chat.ts attaches it.
                uiMessageStreamOptions: {
                    generateMessageId: () => 'reply',
                    onError: (error) => (error instanceof Error ? error.message : String(error)),
                    messageMetadata: ({ part }) =>
                        part.type === 'start' ? { startedAtMs: 1_000 } : undefined,
                },
                retryOptions: { delaysMs: [1], giveUpAfterMs },
            }),
    })
    // Reading a stream keeps each data chunk as its part and updates it in place, so every read gets copies.
    const chunks: UIMessageChunk[] = await convertReadableStreamToArray(
        stream.pipeThrough(
            new TransformStream({
                transform: (chunk, controller) => controller.enqueue(structuredClone(chunk)),
            }),
        ),
    )
    let message: UIMessage | undefined
    for await (message of readUIMessageStream({
        stream: convertArrayToReadableStream(structuredClone(chunks)),
    }));
    return { calls: model.doStreamCalls, chunks, message: message!, outcome, toolRuns }
}
const partTypes = (message: UIMessage) => message.parts.map((part) => part.type)

// Claude refuses a request over its size limit, which would leave the chat failing on every message.
test('The model gets at most 16 MB of images, with a note in place of each older one', async () => {
    const screenshots = Array.from({ length: 17 }, (_, index): ModelMessage[] => [
        {
            role: 'assistant',
            content: [
                { type: 'tool-call', toolCallId: `shot-${index}`, toolName: 'inspect', input: {} },
            ],
        },
        {
            role: 'tool',
            content: [
                {
                    type: 'tool-result',
                    toolCallId: `shot-${index}`,
                    toolName: 'inspect',
                    output: {
                        type: 'content',
                        value: [
                            {
                                type: 'file',
                                mediaType: 'image/jpeg',
                                data: { type: 'data', data: 'A'.repeat(1024 * 1024) },
                            },
                        ],
                    },
                },
            ],
        },
    ]).flat()
    const { calls } = await runReply([textCall('Done', 1_000)], {
        messages: [...screenshots, { role: 'user', content: 'Keep going' }],
    })
    const sentParts = calls[0]!.prompt.flatMap((message) =>
        message.role !== 'tool'
            ? []
            : message.content.flatMap((part) =>
                  part.type === 'tool-result' && part.output.type === 'content'
                      ? part.output.value
                      : [],
              ),
    )
    assert.deepEqual(
        sentParts.map((part) => part.type),
        [...Array.from({ length: 8 }, () => 'text'), ...Array.from({ length: 9 }, () => 'file')],
    )
})

test('Below the limit the reply is one run with no summary', async () => {
    const { calls, chunks, message } = await runReply([
        toolCall('call-1', 219_999),
        textCall('Done', 219_999),
    ])
    assert.equal(calls.length, 2)
    assert.deepEqual(partTypes(message), ['step-start', 'dynamic-tool', 'step-start', 'text'])
    assert.equal(chunks.at(-1)?.type, 'finish')
})

test('A step whose stream fails partway is run again and the reply finishes', async () => {
    const { calls, message, outcome } = await runReply([
        toolCall('call-1', 1_000),
        brokenStreamCall,
        textCall('Done', 1_000),
    ])
    assert.equal(calls.length, 3)
    assert.equal(outcome?.status, 'completed')
    assert.ok(message.parts.some((part) => part.type === 'text' && part.text === 'Done'))
})

test('Reasoning reaches the chat as text, without its signature', async () => {
    const { chunks, message } = await runReply([reasoningCall])
    const reasoning = message.parts.find((part) => part.type === 'reasoning')
    assert.equal(reasoning?.text, 'Planning the track')
    assert.equal(reasoning?.providerMetadata, undefined)
    assert.ok(!JSON.stringify(chunks).includes('signature'))
})

test("GPT's encrypted reasoning never reaches the chat", async () => {
    const { chunks, message } = await runReply([gptReasoningCall])
    assert.ok(!JSON.stringify(chunks).includes('encrypted'))
    assert.deepEqual(
        message.parts.filter((part) => part.type === 'reasoning'),
        [{ type: 'reasoning', id: 'rs_1', text: '', providerMetadata: undefined, state: 'done' }],
    )
})

test('At the limit the reply is summarized and continues from the summary alone', async () => {
    const { calls, chunks, message, outcome } = await runReply([
        toolCall('call-1', 100_000),
        toolCall('call-2', 220_000),
        textCall('Summary of the track', 300_000),
        textCall('Done', 1_000),
    ])
    // The summary call reads 300K tokens and is never itself compacted.
    assert.equal(calls.length, 4)
    const [, lastCall, summaryCall, continueCall] = calls
    assert.deepEqual(summaryCall!.prompt.slice(0, lastCall!.prompt.length), lastCall!.prompt)
    assert.deepEqual(
        summaryCall!.prompt.slice(lastCall!.prompt.length).map((message) => message.role),
        ['assistant', 'tool', 'user'],
    )
    // Every call, the summary's included, tells the model when the conversation is compacted.
    for (const call of calls) {
        assert.equal(call.prompt[0]!.role, 'system')
        assert.match(String(call.prompt[0]!.content), /^System\n\n.*220,000 input tokens/)
    }
    assert.deepEqual(summaryCall!.toolChoice, lastCall!.toolChoice)
    assert.deepEqual(summaryCall!.tools, lastCall!.tools)
    assert.deepEqual(summaryCall!.providerOptions, lastCall!.providerOptions)
    assert.deepEqual(
        continueCall!.prompt.map((message) => message.role),
        ['system', 'user', 'user'],
    )
    assert.deepEqual(continueCall!.prompt[1], {
        role: 'user',
        content: [{ type: 'text', text: summaryText('Summary of the track') }],
        providerOptions: undefined,
    })
    assert.deepEqual(partTypes(message), [
        'step-start',
        'dynamic-tool',
        'step-start',
        'dynamic-tool',
        'data-compaction',
        'step-start',
        'text',
    ])
    const id = (message.parts[4] as { id: string }).id
    assert.deepEqual(message.parts[4], {
        type: 'data-compaction',
        id,
        data: { summary: 'Summary of the track' },
    })
    // The browser shows "Compacting" while the part has no summary yet.
    assert.deepEqual(
        chunks.filter((chunk) => chunk.type === 'data-compaction'),
        [
            { type: 'data-compaction', id, data: {} },
            { type: 'data-compaction', id, data: { summary: 'Summary of the track' } },
        ],
    )
    assert.equal(chunks.filter((chunk) => chunk.type === 'start').length, 1)
    assert.equal(chunks.filter((chunk) => chunk.type === 'finish').length, 1)
    assert.equal(chunks.at(-1)?.type, 'finish')
    assert.deepEqual(outcome, { status: 'completed' })
})

test('The reply carries its start time once, and keeps it through a compaction', async () => {
    const { chunks, message } = await runReply([
        toolCall('call-1', 220_000),
        textCall('Summary of the track', 5_000),
        textCall('Done', 1_000),
    ])
    assert.deepEqual(message.metadata, { startedAtMs: 1_000 })
    assert.equal(chunks.filter((chunk) => 'messageMetadata' in chunk).length, 1)
})

test('One reply can compact more than once', async () => {
    const { calls, message } = await runReply([
        toolCall('call-1', 220_000),
        textCall('First summary', 230_000),
        toolCall('call-2', 225_000),
        textCall('Second summary', 240_000),
        textCall('Done', 1_000),
    ])
    assert.equal(calls.length, 5)
    assert.deepEqual(calls[3]!.prompt.slice(0, calls[2]!.prompt.length), calls[2]!.prompt)
    assert.deepEqual(calls[4]!.prompt[1], {
        role: 'user',
        content: [{ type: 'text', text: summaryText('Second summary') }],
        providerOptions: undefined,
    })
    assert.deepEqual(
        message.parts.flatMap((part) => (part.type === 'data-compaction' ? [part.data] : [])),
        [{ summary: 'First summary' }, { summary: 'Second summary' }],
    )
})

test('A reply that finishes at the limit is summarized for the next message without continuing', async () => {
    const { calls, chunks, message } = await runReply([
        textCall('Done', 230_000),
        textCall('Summary', 5_000),
    ])
    assert.equal(calls.length, 2)
    assert.deepEqual(partTypes(message), ['step-start', 'text', 'data-compaction'])
    assert.equal(chunks.at(-1)?.type, 'finish')
})

test('A tool call during the summary does not run the tool', async () => {
    const { calls, outcome, toolRuns } = await runReply([
        toolCall('call-1', 220_000),
        toolCall('stray', 5_000),
        textCall('Summary', 5_000),
        textCall('Done', 1_000),
    ])
    assert.equal(calls.length, 4)
    assert.equal(toolRuns, 1)
    assert.deepEqual(outcome, { status: 'completed' })
})

test('A reply that finishes at the limit without credits is not summarized', async () => {
    let checks = 0
    const { calls, chunks, message } = await runReply([textCall('Done', 230_000)], {
        shouldStop: async () => {
            checks++
            return true
        },
    })
    assert.equal(calls.length, 1)
    assert.equal(checks, 1)
    assert.deepEqual(partTypes(message), ['step-start', 'text'])
    assert.equal(chunks.at(-1)?.type, 'finish')
})

test('The summary is retried, and only a summary that keeps failing fails the reply', async () => {
    const retried = await runReply([
        toolCall('call-1', 220_000),
        failingCall,
        failingCall,
        textCall('Summary', 5_000),
        textCall('Done', 1_000),
    ])
    assert.equal(retried.calls.length, 5)
    assert.deepEqual(retried.outcome, { status: 'completed' })
    const failed = await runReply([toolCall('call-1', 220_000)], { giveUpAfterMs: 30 })
    assert.ok(failed.calls.length > 2)
    assert.equal(failed.outcome?.status, 'failed')
    assert.deepEqual(failed.chunks.at(-1), {
        type: 'error',
        errorText: 'Could not compact the conversation: Bedrock is down',
    })
})

test('A failed model call fails the reply with its own error', async () => {
    const { chunks, outcome } = await runReply([failingCall])
    assert.equal(outcome?.status, 'failed')
    assert.deepEqual(
        chunks.filter((chunk) => chunk.type === 'error'),
        [{ type: 'error', errorText: 'Bedrock is down' }],
    )
})

test('Stopping the reply during the summary stops without retrying or failing', async () => {
    const controller = new AbortController()
    const { calls, chunks, outcome } = await runReply(
        [
            toolCall('call-1', 220_000),
            ({ abortSignal }) => {
                controller.abort()
                throw abortSignal?.reason
            },
        ],
        { controller },
    )
    assert.equal(calls.length, 2)
    assert.deepEqual(outcome, { status: 'aborted' })
    assert.equal(
        chunks.some((chunk) => chunk.type === 'error'),
        false,
    )
})

// -------------------------- Claude's real provider --------------------------
// The mock model above sees the call before Claude's provider turns it into a request, so these run the real provider
// against a local server that answers the way Bedrock does.
const claudeStream = (inputTokens: number, blocks: { type: string }[][], stopReason: string) =>
    [
        {
            type: 'message_start',
            message: {
                id: `msg_${inputTokens}`,
                type: 'message',
                role: 'assistant',
                model: 'claude-opus-5-5',
                content: [],
                stop_reason: null,
                usage: { input_tokens: inputTokens, output_tokens: 1 },
            },
        },
        ...blocks.flatMap((events, index) => events.map((event) => ({ ...event, index }))),
        { type: 'message_delta', delta: { stop_reason: stopReason }, usage: { output_tokens: 10 } },
        { type: 'message_stop' },
    ]
        .map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`)
        .join('')
const claudeThinking = [
    { type: 'content_block_start', content_block: { type: 'thinking', thinking: '' } },
    {
        type: 'content_block_delta',
        delta: { type: 'thinking_delta', thinking: 'Planning the track' },
    },
    { type: 'content_block_delta', delta: { type: 'signature_delta', signature: 'signature' } },
    { type: 'content_block_stop' },
]
const claudeToolCall = [
    {
        type: 'content_block_start',
        content_block: { type: 'tool_use', id: 'toolu_1', name: 'inspect', input: {} },
    },
    { type: 'content_block_delta', delta: { type: 'input_json_delta', partial_json: '{}' } },
    { type: 'content_block_stop' },
]
const claudeText = (text: string) => [
    { type: 'content_block_start', content_block: { type: 'text', text: '' } },
    { type: 'content_block_delta', delta: { type: 'text_delta', text } },
    { type: 'content_block_stop' },
]
const claudeRequestSchema = z.object({
    tools: z.unknown().optional(),
    tool_choice: z.unknown().optional(),
    system: z.unknown().optional(),
    thinking: z.unknown().optional(),
    messages: z.array(z.unknown()),
})

test("Claude's summary call sends the reply's tools and signed thinking, so it reads the reply's cache", async () => {
    const responses = [
        claudeStream(220_000, [claudeThinking, claudeToolCall], 'tool_use'),
        claudeStream(5_000, [claudeText('Summary of the track')], 'end_turn'),
        claudeStream(1_000, [claudeText('Done')], 'end_turn'),
    ]
    const requests: z.infer<typeof claudeRequestSchema>[] = []
    const server = createServer(async (request, response) => {
        let body = ''
        for await (const chunk of request) body += chunk
        requests.push(claudeRequestSchema.parse(JSON.parse(body)))
        response.writeHead(200, { 'Content-Type': 'text/event-stream' })
        response.end(responses.shift())
    })
    server.listen(0, '127.0.0.1')
    await once(server, 'listening')
    let chunks: UIMessageChunk[]
    try {
        const address = server.address()
        assert(address !== null && typeof address === 'object')
        const modelId = 'anthropic.claude-opus-5-5'
        const stream = createUIMessageStream({
            execute: ({ writer }) =>
                streamCompactingReply({
                    writer,
                    call: {
                        model: createAnthropic({
                            baseURL: `http://127.0.0.1:${address.port}/v1`,
                            apiKey: 'test-only',
                        })(modelId),
                        providerOptions: getModelRoute(modelId).providerOptions,
                        system: 'System',
                        tools: {
                            inspect: dynamicTool({
                                description: 'Inspect the world',
                                inputSchema: jsonSchema({ type: 'object', properties: {} }),
                                execute: async () => 'A red car',
                            }),
                        },
                    },
                    compactionInputTokens: 220_000,
                    signal: new AbortController().signal,
                    messages: [{ role: 'user', content: 'Build a race track' }],
                    shouldStop: async () => false,
                    uiMessageStreamOptions: {
                        generateMessageId: () => 'reply',
                        sendReasoning: true,
                        onError: (error) =>
                            error instanceof Error ? error.message : String(error),
                    },
                    retryOptions: { delaysMs: [1], giveUpAfterMs: 5000 },
                }),
        })
        chunks = await convertReadableStreamToArray(stream)
    } finally {
        server.close()
    }
    assert.equal(requests.length, 3)
    const [replyRequest, summaryRequest, continueRequest] = requests
    assert.deepEqual(summaryRequest!.tools, replyRequest!.tools)
    assert.deepEqual(summaryRequest!.tool_choice, replyRequest!.tool_choice)
    assert.deepEqual(summaryRequest!.system, replyRequest!.system)
    assert.deepEqual(summaryRequest!.thinking, replyRequest!.thinking)
    // Claude needs the signed thinking that came with a tool call when it reads that call again.
    assert.ok(JSON.stringify(summaryRequest!.messages[1]).includes('"signature":"signature"'))
    assert.ok(JSON.stringify(continueRequest!.messages[0]).includes('Summary of the track'))
    // The chat shows the thinking but never stores its signature.
    assert.ok(
        chunks.some(
            (chunk) => chunk.type === 'reasoning-delta' && chunk.delta === 'Planning the track',
        ),
    )
    assert.ok(!JSON.stringify(chunks).includes('signature'))
})
