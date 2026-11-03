import { test } from 'node:test'
import assert from 'node:assert/strict'
import { APICallError, streamText } from 'ai'
import { MockLanguageModelV4, convertArrayToReadableStream } from 'ai/test'

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
const { trackModelCalls } = await import('../src/helpers/stoppedModelCalls.ts')
const { getModelRoute } = await import('../src/helpers/model.ts')
// Each model call starts a 30 minute stall timer. One left behind fires into a dead stream, which crashes the worker.
const countTimers = () =>
    process.getActiveResourcesInfo().filter((resource) => resource === 'Timeout').length
const waitUntil = async (isDone: () => boolean) => {
    for (let turn = 0; turn < 1000 && !isDone(); turn++)
        await new Promise((resolve) => setImmediate(resolve))
}
const trackCalls = (doStream: MockLanguageModelV4['doStream']) =>
    trackModelCalls(
        {
            ...getModelRoute('anthropic.claude-opus-5-5'),
            model: new MockLanguageModelV4({ doStream }),
        },
        'profile',
        null,
    )

test('A stopped call leaves no stall timer running', async () => {
    const timersBefore = countTimers()
    // Bedrock names the call and then sends nothing, like a model thinking silently. Stop ends it the way fetch does.
    const modelCalls = trackCalls(async ({ abortSignal }) => ({
        stream: new ReadableStream({
            start: (stream) => {
                stream.enqueue({ type: 'stream-start', warnings: [] })
                stream.enqueue({ type: 'response-metadata', id: 'call-1' })
                abortSignal?.addEventListener('abort', () => stream.error(abortSignal.reason))
            },
        }),
    }))
    const controller = new AbortController()
    const result = streamText({
        model: modelCalls.model,
        prompt: 'Think',
        abortSignal: controller.signal,
    })
    const reading = (async () => {
        for await (const _part of result.fullStream);
    })()
    await waitUntil(() => countTimers() > timersBefore)
    assert.equal(countTimers(), timersBefore + 1)
    controller.abort()
    await reading
    await waitUntil(() => countTimers() === timersBefore)
    assert.equal(countTimers(), timersBefore)
})

test('A call whose stream is run again leaves no stall timer running', async () => {
    const timersBefore = countTimers()
    let callCount = 0
    // The first call reports an error with its connection still open, so the retry cancels it.
    const modelCalls = trackCalls(async () =>
        callCount++ === 0
            ? {
                  stream: new ReadableStream({
                      start: (stream) => {
                          stream.enqueue({ type: 'stream-start', warnings: [] })
                          stream.enqueue({ type: 'text-start', id: 'partial' })
                          stream.enqueue({ type: 'text-delta', id: 'partial', delta: 'Half a' })
                          stream.enqueue({
                              type: 'error',
                              error: { type: 'api_error', message: 'Internal server error' },
                          })
                      },
                  }),
              }
            : {
                  stream: convertArrayToReadableStream([
                      { type: 'stream-start', warnings: [] },
                      { type: 'text-start', id: 'done' },
                      { type: 'text-delta', id: 'done', delta: 'Done' },
                      { type: 'text-end', id: 'done' },
                      {
                          type: 'finish',
                          finishReason: { unified: 'stop', raw: 'stop' },
                          usage: {
                              inputTokens: { total: 10, noCache: 10, cacheRead: 0, cacheWrite: 0 },
                              outputTokens: { total: 1, text: 1, reasoning: 0 },
                          },
                      },
                  ]),
              },
    )
    const result = streamText({ model: modelCalls.model, prompt: 'Build', streamRetries: 1 })
    assert.equal(await result.text, 'Done')
    await waitUntil(() => countTimers() === timersBefore)
    assert.equal(countTimers(), timersBefore)
})

// Game 330's reply once failed with "Failed to process successful response": Bedrock answered 200 with a body the SDK
// could not read, and the SDK never retries a 200.
test('A call answered with a response that could not be read is run again', async (t) => {
    t.mock.method(console, 'error', () => undefined)
    let callCount = 0
    const modelCalls = trackCalls(async () => {
        if (callCount++ === 0)
            throw new APICallError({
                message: 'Failed to process successful response',
                url: 'https://bedrock.example',
                requestBodyValues: {},
                statusCode: 200,
                cause: new Error('Response body is empty'),
            })
        return {
            stream: convertArrayToReadableStream([
                { type: 'stream-start', warnings: [] },
                { type: 'text-start', id: 'done' },
                { type: 'text-delta', id: 'done', delta: 'Done' },
                { type: 'text-end', id: 'done' },
                {
                    type: 'finish',
                    finishReason: { unified: 'stop', raw: 'stop' },
                    usage: {
                        inputTokens: { total: 10, noCache: 10, cacheRead: 0, cacheWrite: 0 },
                        outputTokens: { total: 1, text: 1, reasoning: 0 },
                    },
                },
            ]),
        }
    })
    const result = streamText({ model: modelCalls.model, prompt: 'Build', maxRetries: 1 })
    assert.equal(await result.text, 'Done')
    assert.equal(callCount, 2)
})
