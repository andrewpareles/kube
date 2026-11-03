import { setTimeout as sleep } from 'node:timers/promises'
import type { LanguageModelUsage } from 'ai'
import { database } from './database.ts'
import type { ModelRoute } from './model.ts'

// Half the model cost, below cost on purpose until the price analysis.
const MARGIN = 0.5
// packages/website/lib/pricing.ts USD_PER_CREDIT, in cents. The two must agree.
const CENTS_PER_CREDIT = 1

export type CallTokens = { noCache: number; cacheRead: number; cacheWrite: number; output: number }

// Integer arithmetic until the final division, so an exact number of credits is never rounded up.
export const getCreditsOfTokens = (
    modelRoute: ModelRoute,
    { noCache, cacheRead, cacheWrite, output }: CallTokens,
) => {
    const centsPerMillionTokens = modelRoute.getCentsPerMillionTokens(
        noCache + cacheRead + cacheWrite,
    )
    const centsTimesMillion =
        noCache * centsPerMillionTokens.noCache +
        cacheRead * centsPerMillionTokens.cacheRead +
        cacheWrite * centsPerMillionTokens.cacheWrite +
        output * centsPerMillionTokens.output
    return Math.ceil((centsTimesMillion * MARGIN) / (1_000_000 * CENTS_PER_CREDIT))
}

// One row and one charge per model call, keyed on its response id (see migration 0048), so a call
// recorded twice is charged once. A charge lands after its call ran, so the last call of a day can
// take what the account used past CREDITS_PER_DAY.
export const recordModelCall = async ({
    modelRoute,
    profileId,
    messageId,
    responseId,
    tokens,
    isOutputKnown,
    credits,
}: {
    modelRoute: ModelRoute
    profileId: string
    messageId: string | null
    responseId: string
    tokens: CallTokens
    isOutputKnown: boolean
    credits: number
}) => {
    await database.chargeChatUsage({
        profileId,
        messageId,
        modelId: modelRoute.modelId,
        inputTokens: tokens.noCache,
        cacheWriteTokens: tokens.cacheWrite,
        cacheReadTokens: tokens.cacheRead,
        outputTokens: isOutputKnown ? tokens.output : null,
        awsRequestId: responseId,
        credits,
    })
}

// Main chat and subagent calls use the same accounting.
export const recordChatUsage = async (
    modelRoute: ModelRoute,
    profileId: string,
    messageId: string | null,
    usage: LanguageModelUsage,
    responseId: string,
) => {
    if (usage.inputTokens === undefined) return
    const tokens = {
        noCache: usage.inputTokenDetails.noCacheTokens ?? usage.inputTokens,
        cacheRead: usage.inputTokenDetails.cacheReadTokens ?? 0,
        cacheWrite: usage.inputTokenDetails.cacheWriteTokens ?? 0,
        output: usage.outputTokens ?? 0,
    }
    await recordModelCall({
        modelRoute,
        profileId,
        messageId,
        responseId,
        tokens,
        isOutputKnown: usage.outputTokens !== undefined,
        credits: getCreditsOfTokens(modelRoute, tokens),
    })
}

// TODO: AI-PICKED-VALUE: A failed credit read is tried again after 1, 2 and 4 seconds, so one database hiccup doesn't stop a reply.
const CREDIT_READ_RETRY_DELAYS_MS = [1000, 2000, 4000]

// What every account can use a day. Nobody is shown it.
export const CREDITS_PER_DAY = 2_000

export const hasEnoughCredits = async (profileId: string) => {
    for (const delayMs of CREDIT_READ_RETRY_DELAYS_MS) {
        try {
            return (await database.creditsUsedToday(profileId)) < CREDITS_PER_DAY
        } catch (error) {
            console.error(`Could not read credits, trying again in ${delayMs}ms`, error)
            await sleep(delayMs)
        }
    }
    return (await database.creditsUsedToday(profileId)) < CREDITS_PER_DAY
}
