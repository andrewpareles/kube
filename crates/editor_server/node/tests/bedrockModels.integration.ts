import { test } from 'node:test'
import assert from 'node:assert/strict'
import { setTimeout as sleep } from 'node:timers/promises'
import { setFlagsFromString } from 'node:v8'
import { runInNewContext } from 'node:vm'
import { streamText, wrapLanguageModel } from 'ai'

// Run explicitly with npm run test:bedrock. It calls the real models with the Bedrock API key in
// packages/website/.env.local, so each run costs a few cents. Nothing touches the database.
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
})
const { getModelRoute, modelIds } = await import('../src/helpers/model.ts')

setFlagsFromString('--expose-gc')
const collectGarbage: () => void = runInNewContext('gc')

const THINKING_PROMPT =
    'How many integers from 1 to 1,000,000 are divisible by 3, 5 or 7 but by none of 11, 13 and 17? Work it out carefully, then reply with just the number.'

for (const modelId of modelIds) {
    test(`${modelId} answers with the Bedrock API key`, async () => {
        const route = getModelRoute(modelId)
        const result = streamText({
            model: route.model,
            providerOptions: route.providerOptions,
            prompt: 'Reply with just the word ok.',
        })
        assert.match((await result.text).toLowerCase(), /ok/)
    })

    // Stop is pressed a second into thinking, after garbage is collected, as in a real reply. The call must end at once,
    // not when the next piece of the stream happens to arrive. Claude's thinking summary is hidden here, because the AI SDK
    // would notice Stop at its next piece even if Stop never reached the connection. GPT sends nothing while it reasons.
    test(`Stop ends a ${modelId} call while it thinks, and the stopped call can be charged`, async () => {
        const route = getModelRoute(modelId)
        const { promise: responseIdPromise, resolve: resolveResponseId } =
            Promise.withResolvers<string>()
        const controller = new AbortController()
        const result = streamText({
            model: wrapLanguageModel({
                model: route.model,
                middleware: {
                    wrapStream: async ({ doStream }) => {
                        const streamed = await doStream()
                        return {
                            ...streamed,
                            stream: streamed.stream.pipeThrough(
                                new TransformStream({
                                    transform(part, stream) {
                                        if (part.type === 'response-metadata' && part.id)
                                            resolveResponseId(part.id)
                                        stream.enqueue(part)
                                    },
                                }),
                            ),
                        }
                    },
                },
            }),
            providerOptions: {
                ...route.providerOptions,
                anthropic: {
                    ...route.providerOptions.anthropic,
                    thinking: { type: 'adaptive', display: 'omitted' },
                },
            },
            prompt: THINKING_PROMPT,
            abortSignal: controller.signal,
        })
        let abortedAt = Number.POSITIVE_INFINITY
        const stopping = responseIdPromise.then(async () => {
            await sleep(1000)
            collectGarbage()
            abortedAt = performance.now()
            controller.abort()
        })
        const partTypes: string[] = []
        for await (const part of result.fullStream) partTypes.push(part.type)
        const endedAt = performance.now()
        await stopping

        assert.ok(
            partTypes.includes('abort'),
            `the call finished before Stop: ${partTypes.join(', ')}`,
        )
        assert.ok(endedAt - abortedAt < 2000, `the call ended ${endedAt - abortedAt}ms after Stop`)
        const stoppedCall = await route.readStoppedCall(await responseIdPromise)
        assert.ok(stoppedCall)
        const { noCache, cacheRead, cacheWrite } = stoppedCall.tokens
        assert.ok(noCache + cacheRead + cacheWrite > 0)
        assert.equal(stoppedCall.isOutputKnown, false)
    })
}
