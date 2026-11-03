import { randomUUID } from 'node:crypto'
import { setTimeout as sleep } from 'node:timers/promises'
import {
    convertToModelMessages,
    streamText,
    type LanguageModelUsage,
    type ModelMessage,
    type ToolSet,
    type UIMessage,
    type UIMessageChunk,
    type UIMessageStreamOptions,
    type UIMessageStreamOutcome,
    type UIMessageStreamWriterWithOutcome,
} from 'ai'
import { z } from 'zod'
import { keepImagesWithinBudget } from './imageBudget.ts'
import { MODEL_MAX_RETRIES, MODEL_STREAM_RETRIES } from './model.ts'

// TODO: AI-PICKED-VALUE: Retry a failed summary after 1s, 2s, 4s, 8s and 16s, then every 30s, and give up 5 minutes after the first failure.
const SUMMARY_RETRY_DELAYS_MS = [1000, 2000, 4000, 8000, 16_000, 30_000]
const SUMMARY_RETRY_MS = 5 * 60_000
// TODO: AI-PICKED-VALUE: Far longer than a summary takes, so only a hung call is cut off and retried.
const SUMMARY_TIMEOUT_MS = 10 * 60_000

const SUMMARY_REQUEST = `Your context is almost full, so this conversation is being compacted. Write a summary that will replace everything above. You will continue from the summary alone, with the same tools, so include everything you need to carry on:
- the user's goals and preferences
- the user's latest request
- if you had just finished your reply, what it told the user, keeping any options, lists or questions word for word
- what has been built or changed, naming each object with its id
- key decisions and why they were made
- open problems
- what you were in the middle of, and the next steps
If the conversation began with an earlier summary, carry forward what still matters from it. Leave out details of the world that you can inspect again with tools. Don't call any tools. Reply with only the summary.`
const CONTINUE_REQUEST =
    'The conversation was compacted into the summary above. Continue the reply you were in the middle of from where you left off, without repeating work that is already done.'
const getCompactionNote = (compactionInputTokens: number) =>
    `This conversation is compacted once a call reads ${compactionInputTokens.toLocaleString('en-US')} input tokens: you will summarize it, then continue from the summary alone.`

export const isCompactionNeeded = (usage: LanguageModelUsage, compactionInputTokens: number) =>
    (usage.inputTokens ?? 0) >= compactionInputTokens

const compactionSchema = z.object({ summary: z.string() })
const getSummary = (part: UIMessage['parts'][number]) =>
    part.type === 'data-compaction'
        ? compactionSchema.safeParse(part.data).data?.summary
        : undefined
const getSummaryMessage = (summary: string): ModelMessage => ({
    role: 'user',
    content: `<conversation_summary>\n${summary}\n</conversation_summary>`,
})

// The model starts from the latest summary: the summary, the rest of the reply it was written in, and every later message.
// Reasoning is only shown in the chat, so the model gets earlier messages without it.
export const toModelInput = async (
    shownMessages: UIMessage[],
    options: Parameters<typeof convertToModelMessages>[1],
) => {
    const messages = shownMessages.map((message) => ({
        ...message,
        parts: message.parts.filter((part) => part.type !== 'reasoning'),
    }))
    for (let index = messages.length - 1; index >= 0; index--) {
        const message = messages[index]!
        const partIndex = message.parts.findLastIndex((part) => getSummary(part) !== undefined)
        if (partIndex === -1) continue
        return [
            getSummaryMessage(getSummary(message.parts[partIndex]!)!),
            ...(await convertToModelMessages(
                [
                    { ...message, parts: message.parts.slice(partIndex + 1) },
                    ...messages.slice(index + 1),
                ],
                options,
            )),
        ]
    }
    return await convertToModelMessages(messages, options)
}

export const retryWithBackoff = async <T>(
    attempt: () => Promise<T>,
    signal: AbortSignal,
    { delaysMs = SUMMARY_RETRY_DELAYS_MS, giveUpAfterMs = SUMMARY_RETRY_MS } = {},
) => {
    let firstFailedAt: number | undefined
    for (let failures = 0; ; failures++) {
        try {
            return await attempt()
        } catch (error) {
            signal.throwIfAborted()
            firstFailedAt ??= Date.now()
            const delayMs = delaysMs[Math.min(failures, delaysMs.length - 1)]!
            if (Date.now() - firstFailedAt + delayMs > giveUpAfterMs) throw error
            console.error(`Could not compact the conversation, retrying in ${delayMs}ms`, error)
            await sleep(delayMs, undefined, { signal })
        }
    }
}

type StreamTextOptions = Parameters<typeof streamText>[0]
// The summary call sends exactly what the reply's calls send, so it reads their prompt cache.
type ReplyCall = Pick<
    StreamTextOptions,
    'model' | 'providerOptions' | 'tools' | 'onLanguageModelCallEnd'
> & { system: string }

const summarize = async (call: ReplyCall, messages: ModelMessage[], signal: AbortSignal) => {
    // Without execute, a tool call the model makes anyway only fails the attempt instead of editing the world.
    const tools: ToolSet = Object.fromEntries(
        Object.entries(call.tools ?? {}).map(([name, tool]) => [
            name,
            { ...tool, execute: undefined },
        ]),
    )
    const result = streamText({
        ...call,
        tools,
        messages: [...messages, { role: 'user', content: SUMMARY_REQUEST }],
        abortSignal: signal,
        maxRetries: MODEL_MAX_RETRIES,
        streamRetries: MODEL_STREAM_RETRIES,
        timeout: { totalMs: SUMMARY_TIMEOUT_MS },
    })
    for await (const part of result.fullStream) if (part.type === 'error') throw part.error
    const summary = (await result.text).trim()
    if (!summary) throw new Error('The summary was empty.')
    return summary
}

// One reply can be several streamText runs. A run that reaches the input limit ends after its step, the
// reply is summarized into a data-compaction part, and the next run continues from the summary alone.
export const streamCompactingReply = async ({
    writer,
    call,
    compactionInputTokens,
    signal,
    messages,
    transform,
    shouldStop,
    uiMessageStreamOptions,
    retryOptions,
}: {
    writer: UIMessageStreamWriterWithOutcome<UIMessage>
    call: ReplyCall
    compactionInputTokens: number
    signal: AbortSignal
    messages: ModelMessage[]
    transform?: StreamTextOptions['experimental_transform']
    shouldStop: () => Promise<boolean>
    // Without onError the AI SDK replaces every model and tool error with "An error occurred.".
    uiMessageStreamOptions: UIMessageStreamOptions<UIMessage> & {
        onError: (error: unknown) => string
    }
    retryOptions?: Parameters<typeof retryWithBackoff>[2]
}) => {
    // Every call, the summary's included, tells the model when the conversation is compacted.
    const replyCall = {
        ...call,
        system: `${call.system}\n\n${getCompactionNote(compactionInputTokens)}`,
    }
    let runMessages = messages
    for (let run = 0; ; run++) {
        let isStoppedByCaller = false
        const result = streamText({
            ...replyCall,
            messages: runMessages,
            experimental_transform: transform,
            abortSignal: signal,
            maxRetries: MODEL_MAX_RETRIES,
            streamRetries: MODEL_STREAM_RETRIES,
            stopWhen: [
                ({ steps }) => isCompactionNeeded(steps.at(-1)!.usage, compactionInputTokens),
                async () => (isStoppedByCaller = await shouldStop()),
            ],
            prepareStep: ({ initialMessages, responseMessages }) => ({
                messages: keepImagesWithinBudget([...initialMessages, ...responseMessages]),
            }),
        })
        let outcome = { status: 'unknown' } as UIMessageStreamOutcome
        // Only the reply's first run starts the message and only its last run finishes it.
        let finish: UIMessageChunk | undefined
        for await (const chunk of result.toUIMessageStream({
            ...uiMessageStreamOptions,
            sendStart: run === 0,
            onEnd: (event) => {
                outcome = event.outcome
            },
        })) {
            if (chunk.type === 'finish') finish = chunk
            // The model never reads reasoning back from the chat, so its signature is not saved with the reply.
            else if (
                chunk.type === 'reasoning-start' ||
                chunk.type === 'reasoning-delta' ||
                chunk.type === 'reasoning-end'
            )
                writer.write({ ...chunk, providerMetadata: undefined })
            else writer.write(chunk)
        }
        const steps = outcome.status === 'completed' ? await result.steps : []
        const lastStep = steps.at(-1)
        // Stop conditions only run after a step with tool calls, so a final text step has not checked credits.
        if (
            !lastStep ||
            !isCompactionNeeded(lastStep.usage, compactionInputTokens) ||
            (lastStep.toolCalls.length === 0 ? await shouldStop() : isStoppedByCaller)
        ) {
            if (finish) writer.write(finish)
            return writer.setOutcome(outcome)
        }
        const id = randomUUID()
        writer.write({ type: 'data-compaction', id, data: {} })
        const history = [...runMessages, ...(await result.responseMessages)]
        let summary: string
        try {
            summary = await retryWithBackoff(
                () => summarize(replyCall, keepImagesWithinBudget(history), signal),
                signal,
                retryOptions,
            )
        } catch (error) {
            if (signal.aborted) return writer.setOutcome({ status: 'aborted' })
            throw new Error(
                `Could not compact the conversation: ${error instanceof Error ? error.message : String(error)}`,
            )
        }
        writer.write({ type: 'data-compaction', id, data: { summary } })
        // A run that ended without tool calls had finished the reply, so the summary only serves the next message.
        if (lastStep.toolCalls.length === 0 || (await shouldStop())) {
            if (finish) writer.write(finish)
            return writer.setOutcome(outcome)
        }
        runMessages = [getSummaryMessage(summary), { role: 'user', content: CONTINUE_REQUEST }]
    }
}
