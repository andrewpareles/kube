import { createAnthropic } from '@ai-sdk/anthropic'
import { wrapLanguageModel } from 'ai'
import { z } from 'zod'
import type { CallTokens } from '../chatUsage.ts'
import { env } from '../env.ts'
import type { ModelId, ModelRoute } from '../model.ts'

// Claude on Bedrock mantle, through Anthropic's own Messages API with a Bedrock API key.

// Claude Opus 5.5 on Bedrock mantle (regional, not global), in US cents per million tokens. The cache
// write is the 5-minute one. No long-context surcharge.
const centsPerMillionTokens = { noCache: 440, cacheRead: 22, cacheWrite: 550, output: 2200 }

// Claude reports the call's input in the first event of its stream and its output in the last. A
// cut-off call is charged for the input it reported and nothing for its output; one cut off after the
// last event is charged in full. A call cut off before its first event is not charged.
const streamEventSchema = z.discriminatedUnion('type', [
    z.object({
        type: z.literal('message_start'),
        message: z.object({
            id: z.string(),
            usage: z.object({
                input_tokens: z.number(),
                cache_read_input_tokens: z.number().nullish(),
                cache_creation_input_tokens: z.number().nullish(),
            }),
        }),
    }),
    z.object({
        type: z.literal('message_delta'),
        usage: z.object({ output_tokens: z.number() }),
    }),
])

export const createAnthropicRoute = (modelId: ModelId): ModelRoute => {
    const anthropic = createAnthropic({
        baseURL: `https://bedrock-mantle.${env.AWS_REGION}.api.aws/anthropic/v1`,
        apiKey: env.AWS_BEARER_TOKEN_BEDROCK,
    })

    // Calls whose stream has not ended yet, by message id.
    const streamedCalls = new Map<string, { tokens: CallTokens; isOutputKnown: boolean }>()

    return {
        modelId,
        model: wrapLanguageModel({
            model: anthropic(modelId),
            middleware: {
                transformParams: async ({ params }) => ({ ...params, includeRawChunks: true }),
                wrapStream: async ({ doStream }) => {
                    const result = await doStream()
                    let messageId: string | undefined
                    return {
                        ...result,
                        stream: result.stream.pipeThrough(
                            new TransformStream({
                                transform(part, controller) {
                                    if (part.type !== 'raw') {
                                        if (part.type === 'error' && messageId)
                                            streamedCalls.delete(messageId)
                                        controller.enqueue(part)
                                        return
                                    }
                                    const event = streamEventSchema.safeParse(part.rawValue)
                                    if (!event.success) return
                                    if (event.data.type === 'message_start') {
                                        const { id, usage } = event.data.message
                                        messageId = id
                                        streamedCalls.set(id, {
                                            tokens: {
                                                noCache: usage.input_tokens,
                                                cacheRead: usage.cache_read_input_tokens ?? 0,
                                                cacheWrite: usage.cache_creation_input_tokens ?? 0,
                                                output: 0,
                                            },
                                            isOutputKnown: false,
                                        })
                                    } else {
                                        const call = messageId && streamedCalls.get(messageId)
                                        if (!call) return
                                        call.tokens.output = event.data.usage.output_tokens
                                        call.isOutputKnown = true
                                    }
                                },
                                // The whole stream was read, so the call ends normally and is charged from its usage.
                                flush() {
                                    if (messageId) streamedCalls.delete(messageId)
                                },
                            }),
                        ),
                    }
                },
            },
        }),
        // Thinking is always on for Opus 5.5. Its summary streams to the chat, which otherwise shows nothing while it thinks.
        providerOptions: {
            anthropic: {
                effort: 'max',
                thinking: { type: 'adaptive', display: 'summarized' },
                cacheControl: { type: 'ephemeral' },
            },
        },
        getCentsPerMillionTokens: () => centsPerMillionTokens,
        readStoppedCall: async (responseId) => {
            const call = streamedCalls.get(responseId)
            streamedCalls.delete(responseId)
            return call && { ...call, chargedTokens: call.tokens }
        },
    }
}
