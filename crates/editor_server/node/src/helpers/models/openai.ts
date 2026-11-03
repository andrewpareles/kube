import { createBedrockMantle } from '@ai-sdk/amazon-bedrock/mantle'
import { wrapLanguageModel } from 'ai'
import { setTimeout as sleep } from 'node:timers/promises'
import { z } from 'zod'
import { env } from '../env.ts'
import type { ModelId, ModelRoute } from '../model.ts'

// OpenAI models on Bedrock mantle, through the OpenAI Responses API.
const mantleBaseUrl = `https://bedrock-mantle.${env.AWS_REGION}.api.aws/openai/v1`

// GPT-6 Astra on Bedrock mantle, in US cents per million tokens. Above 272K input tokens a call is
// long-context: input costs twice as much and output one and a half times as much.
const centsPerMillionTokens = { noCache: 1100, cacheRead: 110, cacheWrite: 1375, output: 5500 }
const LONG_CONTEXT_INPUT_TOKENS = 272_000

// Bedrock keeps a record of a cut-off call, but the record gets two things wrong: it says 0 output
// tokens even when the model had already written some, and it labels the whole input as a new cache
// write even when the cache was warm, which would bill the input about 12x too high. Only its input
// token total is right. So a cut-off call is charged for its input at the cache-read rate and nothing
// for its output. A call cut off before Bedrock names it (about its first half second) has no record
// to read and is not charged.
const storedResponseSchema = z.object({
    status: z.string(),
    usage: z
        .object({
            input_tokens: z.number(),
            input_tokens_details: z.object({
                cached_tokens: z.number(),
                cache_write_tokens: z.number().nullish(),
            }),
            output_tokens: z.number(),
        })
        .nullish(),
})

export const createOpenAIRoute = (modelId: ModelId): ModelRoute => {
    const mantle = createBedrockMantle({
        apiKey: env.AWS_BEARER_TOKEN_BEDROCK,
        baseURL: mantleBaseUrl,
    })

    // Right after the stop, Bedrock can still be cancelling the call, or be briefly unavailable.
    const readStoredResponse = async (responseId: string) => {
        // TODO: AI-PICKED-VALUE: Wait up to ten seconds for Bedrock to settle the call.
        for (let attempt = 0; attempt < 10; attempt++) {
            const response = await fetch(
                `${mantleBaseUrl}/responses/${encodeURIComponent(responseId)}`,
                { headers: { Authorization: `Bearer ${env.AWS_BEARER_TOKEN_BEDROCK}` } },
            )
            if (response.status === 429 || response.status >= 500) {
                await response.body?.cancel()
                await sleep(1000)
                continue
            }
            if (!response.ok)
                throw new Error(`Bedrock returned ${response.status} for ${responseId}.`)
            const stored = storedResponseSchema.parse(await response.json())
            if (stored.status !== 'queued' && stored.status !== 'in_progress') return stored
            await sleep(1000)
        }
        throw new Error(`Bedrock did not settle ${responseId} in time.`)
    }

    return {
        modelId,
        // The SDK refers to the model's earlier text and reasoning by the id of Bedrock's stored copy, and
        // Bedrock silently drops those references, so the model never saw what it had just said. Without the
        // id, the SDK sends each one in full: text as written, reasoning as the encrypted copy `include` asks for.
        model: wrapLanguageModel({
            model: mantle.responses(modelId),
            middleware: {
                transformParams: async ({ params }) => ({
                    ...params,
                    prompt: params.prompt.map((message) =>
                        message.role !== 'assistant'
                            ? message
                            : {
                                  ...message,
                                  content: message.content.map((part) => {
                                      const { itemId, ...openai } =
                                          part.providerOptions?.openai ?? {}
                                      return itemId === undefined
                                          ? part
                                          : {
                                                ...part,
                                                providerOptions: {
                                                    ...part.providerOptions,
                                                    openai,
                                                },
                                            }
                                  }),
                              },
                    ),
                }),
            },
        }),
        // forceReasoning is needed because the SDK only recognizes unprefixed GPT ids as reasoning models.
        // Bedrock rejects the reasoning summary the SDK asks for by default. store keeps the record
        // readStoppedCall reads.
        providerOptions: {
            openai: {
                reasoningEffort: 'xhigh',
                forceReasoning: true,
                reasoningSummary: null,
                store: true,
                include: ['reasoning.encrypted_content'],
            },
        },
        getCentsPerMillionTokens: (inputTokens) =>
            inputTokens > LONG_CONTEXT_INPUT_TOKENS
                ? {
                      noCache: centsPerMillionTokens.noCache * 2,
                      cacheRead: centsPerMillionTokens.cacheRead * 2,
                      cacheWrite: centsPerMillionTokens.cacheWrite * 2,
                      output: centsPerMillionTokens.output * 1.5,
                  }
                : centsPerMillionTokens,
        readStoppedCall: async (responseId) => {
            const { status, usage } = await readStoredResponse(responseId)
            if (!usage) return undefined
            const cacheRead = usage.input_tokens_details.cached_tokens
            const cacheWrite = usage.input_tokens_details.cache_write_tokens ?? 0
            const tokens = {
                noCache: usage.input_tokens - cacheRead - cacheWrite,
                cacheRead,
                cacheWrite,
                output: usage.output_tokens,
            }
            // The call finished just as the run stopped, so its record is real.
            if (status === 'completed' || status === 'incomplete')
                return { tokens, isOutputKnown: true, chargedTokens: tokens }
            // A failed call is never charged.
            if (status !== 'cancelled') return undefined
            return {
                tokens,
                isOutputKnown: false,
                chargedTokens: {
                    noCache: 0,
                    cacheRead: usage.input_tokens,
                    cacheWrite: 0,
                    output: 0,
                },
            }
        },
    }
}
