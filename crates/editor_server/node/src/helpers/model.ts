import type { streamText, wrapLanguageModel } from 'ai'
import type { CallTokens } from './chatUsage.ts'
import { createAnthropicRoute } from './models/anthropic.ts'
import { createOpenAIRoute } from './models/openai.ts'

// What a call cut off mid-stream is recorded as, and what it is charged for.
export type StoppedCall = { tokens: CallTokens; isOutputKnown: boolean; chargedTokens: CallTokens }

// Everything that differs between Bedrock mantle's OpenAI and Anthropic APIs. A reply's subagents use
// the reply's route, so they share its prompt cache.
export type ModelRoute = {
    modelId: ModelId
    model: ReturnType<typeof wrapLanguageModel>
    providerOptions: NonNullable<Parameters<typeof streamText>[0]['providerOptions']>
    getCentsPerMillionTokens: (inputTokens: number) => CallTokens
    // undefined when the call is not charged.
    readStoppedCall: (responseId: string) => Promise<StoppedCall | undefined>
}

// Each person picks the model for their own messages in the chat.
export const modelIds = ['anthropic.claude-opus-5-5', 'openai.gpt-6-astra'] as const
export type ModelId = (typeof modelIds)[number]
export const DEFAULT_MODEL_ID: ModelId = 'anthropic.claude-opus-5-5'

const modelRouteOfId: Record<ModelId, ModelRoute> = {
    'anthropic.claude-opus-5-5': createAnthropicRoute('anthropic.claude-opus-5-5'),
    'openai.gpt-6-astra': createOpenAIRoute('openai.gpt-6-astra'),
}
export const getModelRoute = (modelId: ModelId) => modelRouteOfId[modelId]
// TODO: AI-PICKED-VALUE: The AI SDK waits 2, 4 then 8 seconds between retries.
export const MODEL_MAX_RETRIES = 3
// TODO: AI-PICKED-VALUE: A step whose stream fails partway, from a Bedrock error or a stall, is run again up to 3 times.
export const MODEL_STREAM_RETRIES = 3
