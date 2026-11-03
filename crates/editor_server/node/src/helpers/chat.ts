import { randomUUID } from 'node:crypto'
import { setTimeout as sleep } from 'node:timers/promises'
import { getModelRoute, modelIds, DEFAULT_MODEL_ID } from './model.ts'
import { hasEnoughCredits } from './chatUsage.ts'
import { trackModelCalls } from './stoppedModelCalls.ts'
import { getMessageUpdate } from './messageUpdates.ts'
import { streamCompactingReply, toModelInput } from './compaction.ts'
import {
    APICallError,
    RetryError,
    StreamProviderError,
    createUIMessageStream,
    isToolUIPart,
    readUIMessageStream,
    smoothStream,
    validateUIMessages,
    type JSONSchema7,
    type UIMessage,
} from 'ai'
import { HTTPException } from 'hono/http-exception'
import { z } from 'zod'
import { env } from './env.ts'
import { database } from './database.ts'
import { editorRuntime, workerFetch } from './worker.ts'

import { createChatToolSet, type SubagentProgress } from './chatTools.ts'

import { createEventBroadcast } from './eventBroadcast.ts'

export const chatEvents = createEventBroadcast()

const selectedObjectsSchema = z
    .array(z.object({ objectId: z.int().nonnegative(), name: z.string().max(256), kind: z.string().max(64) }))
    .min(1)
    .max(500)
// The user's attachments reach the model as their own blocks; the turn's object list is for the chat only.
export const convertDataPart = (part: { type: string; data: unknown }) => {
    if (part.type !== 'data-selected-objects') return undefined
    const objects = selectedObjectsSchema.safeParse(part.data)
    if (!objects.success) return undefined
    const lines = objects.data.map((object) =>
        JSON.stringify({ object_id: object.objectId, name: object.name, kind: object.kind }),
    )
    return {
        type: 'text' as const,
        text: `<selected_objects>\nThe user had these objects selected in the editor when sending this message:\n${lines.join('\n')}\n</selected_objects>`,
    }
}

const MODEL_OVERLOADED_REASON = 'The AI model is overloaded right now.'
const isModelOverloaded = (error: unknown): boolean =>
    RetryError.isInstance(error)
        ? isModelOverloaded(error.lastError)
        : (APICallError.isInstance(error) && error.statusCode === 529) ||
          (StreamProviderError.isInstance(error) &&
              (error.statusCode === 529 || error.type === 'overloaded_error'))
const getErrorReason = (error: unknown) =>
    isModelOverloaded(error)
        ? MODEL_OVERLOADED_REASON
        : error instanceof Error && error.message
          ? error.message
          : 'The reply stopped unexpectedly.'
const hasVisibleOutput = (message: UIMessage | undefined) =>
    message?.parts.some(
        (part) =>
            (part.type === 'text' && part.text.trim() !== '') ||
            part.type === 'dynamic-tool' ||
            part.type.startsWith('tool-'),
    ) === true

const objectChangesSchema = z.array(
    z.object({
        objectId: z.number(),
        name: z.string(),
        kind: z.string(),
        change: z.enum(['created', 'modified', 'deleted']),
    }),
)

// This process owns one game's conversation and at most one active reply.
// Only one editor runs per game at a time, so this process's reply is the game's only one.
type ChatTurn = {
    isStopRequested: boolean
    controller: AbortController
    finished: Promise<void>
    // The streaming reply as browsers have it from the updates published so far.
    publishedReply: UIMessage | undefined
}
let currentTurn: ChatTurn | undefined

// The browser receives only saved messages. It cannot replace history or invent successful tool results.
// A browser that connects while a reply streams gets the reply as last published, so the next update applies to it.
// The reply is read before anything awaits, so no update can be published between it and the subscription.
export const getChatState = async () => {
    const publishedReply = currentTurn?.publishedReply
    const savedMessages = await database.chatMessages()
    const messages =
        savedMessages.length === 0 ? [] : await validateUIMessages({ messages: savedMessages })
    return {
        type: 'state',
        messages: messages.map((message) =>
            message.id === publishedReply?.id ? publishedReply : message,
        ),
    }
}
// The model starts from the latest summary, so only the browser needs the messages before it.
const getMessagesSinceSummary = async () => {
    const savedMessages = await database.chatMessagesSinceSummary()
    return savedMessages.length === 0 ? [] : await validateUIMessages({ messages: savedMessages })
}
// TODO: AI-PICKED-VALUE: A failed save is tried three more times, after 1, 2 and 4 seconds.
const saveMessage = async (message: UIMessage) => {
    for (let attempt = 0; ; attempt++) {
        try {
            await database.saveChatMessage(message)
            return
        } catch (error) {
            if (attempt >= 3) throw error
            await sleep(1000 * 2 ** attempt)
        }
    }
}
// A reply still marked isRunning when this process starts was cut off when the editor before it died. It ends the way
// a failed reply ends: removed if it showed nothing, so Retry resumes the user's message, and marked failed otherwise.
export const failRepliesLeftRunning = async () => {
    const runningReplies = await database.runningReplies()
    if (runningReplies.length === 0) return
    for (const reply of await validateUIMessages({ messages: runningReplies })) {
        if (!hasVisibleOutput(reply)) await database.deleteChatMessage(reply.id)
        else
            await saveMessage({
                ...reply,
                metadata: undefined,
                parts: [
                    ...reply.parts,
                    { type: 'data-reply-failed', data: { reason: 'The editor stopped before this reply finished.' } },
                ],
            })
    }
}
const countSteps = (message: UIMessage) =>
    message.parts.filter((part) => part.type === 'step-start').length
export const interruptChat = async () => {
    const turn = currentTurn
    if (turn) turn.isStopRequested = true
    turn?.controller.abort()
    await turn?.finished
}

export const chat = async (profileId: string, body: unknown) => {
    if (editorRuntime.isStopping)
        throw new HTTPException(503, { message: 'The editor is shutting down.' })
    const input = z
        .object({
            message: z.object({
                id: z.string().min(1).max(128),
                role: z.literal('user'),
                parts: z
                    .array(
                        z.discriminatedUnion('type', [
                            z.object({ type: z.literal('text'), text: z.string().min(1).max(100_000) }),
                            z.object({ type: z.literal('data-selected-objects'), data: selectedObjectsSchema }),
                        ]),
                    )
                    .max(20)
                    .refine((parts) => parts.some((part) => part.type === 'text')),
            }),
            model: z.enum(modelIds).default(DEFAULT_MODEL_ID),
        })
        .parse(body)
    if (currentTurn)
        throw new HTTPException(409, { message: 'This game already has an active reply.' })
    const controller = new AbortController()
    const { promise: finished, resolve: resolveFinished } = Promise.withResolvers<void>()
    const responseId = randomUUID()
    const startedAtMs = Date.now()
    const modelRoute = getModelRoute(input.model)
    const turn: ChatTurn = {
        isStopRequested: false,
        controller,
        finished,
        publishedReply: undefined,
    }
    // Every completion path releases the turn and renews browser subscriptions through normal authentication.
    const finishTurn = () => {
        currentTurn = undefined
        chatEvents.closeSubscribers()
        resolveFinished()
    }
    // Reserve the turn before awaiting the database so simultaneous messages cannot queue behind it.
    currentTurn = turn
    let isStreamStarted = false
    let isReplySaved = false
    let didReportChatRunning = false
    // Rust distributes running state to every editor; AI SDK still owns message streaming.
    // Rust answers with the objects the turn changed.
    const reportChatFinished = async () => {
        if (!didReportChatRunning) return []
        try {
            const response = await workerFetch(
                '/local-only/chat-status',
                'POST',
                { is_chat_running: false },
                undefined,
            )
            return objectChangesSchema.parse(await response.json())
        } catch (error) {
            console.error('Could not synchronize chat completion', error)
            return undefined
        }
    }
    try {
        if (editorRuntime.isStopping)
            throw new HTTPException(503, { message: 'The editor is shutting down.' })
        if (!(await hasEnoughCredits(profileId)))
            throw new HTTPException(402, { message: 'Out of credits.' })
        const registry = z
            .object({
                system_prompt: z.string(),
                compaction_input_tokens: z.number(),
                tools: z.array(
                    z.object({
                        name: z.string(),
                        description: z.string(),
                        inputSchema: z.custom<JSONSchema7>(
                            (value) => typeof value === 'object' && value !== null,
                        ),
                    }),
                ),
            })
            .parse(
                await (
                    await workerFetch('/local-only/tools', 'GET', undefined, controller.signal)
                ).json(),
            )
        const history = await getMessagesSinceSummary()
        const lastMessage = history.at(-1)
        const isPendingMessage = lastMessage?.role === 'user' && lastMessage.id === input.message.id
        if (!isPendingMessage && history.some((message) => message.id === input.message.id))
            throw new HTTPException(409, {
                message: 'This message was already submitted. Refresh the chat.',
            })
        // A saved homepage prompt is resumed verbatim; clients cannot rewrite it.
        const userMessage = isPendingMessage ? lastMessage : input.message
        // Saves made while the reply runs mark it isRunning, so an editor that starts after this one dies can fail it.
        await database.startChatReply(isPendingMessage ? null : userMessage, {
            id: responseId,
            role: 'assistant',
            parts: [],
            metadata: { startedAtMs, isRunning: true },
        })
        isReplySaved = true
        chatEvents.publish({ type: 'message', message: userMessage })
        const messages: UIMessage[] = isPendingMessage ? history : [...history, userMessage]
        didReportChatRunning = true
        // Finish this report even if chat is interrupted, so the completion report arrives after it.
        await (
            await workerFetch(
                '/local-only/chat-status',
                'POST',
                { is_chat_running: true },
                undefined,
            )
        ).body?.cancel()
        let isInterrupted = false
        let failureReason: string | undefined
        let isOutOfCredits = false
        const modelCalls = trackModelCalls(modelRoute, profileId, responseId)
        const stream = createUIMessageStream({
            originalMessages: messages,
            generateId: () => responseId,
            onEnd: ({ outcome }) => {
                isInterrupted = outcome.status !== 'completed'
                if (outcome.status === 'failed') failureReason ??= getErrorReason(outcome.error)
            },
            onError: (error) => (failureReason ??= getErrorReason(error)),
            execute: async ({ writer }) => {
                // The reply lists the objects its tools have changed so far, so the list fills in as the AI works.
                const writeObjectChanges = async () => {
                    const response = await workerFetch('/local-only/chat-turn-changes', 'GET', undefined, undefined)
                    const objectChanges = objectChangesSchema.parse(await response.json())
                    if (objectChanges.length > 0)
                        writer.write({ type: 'data-object-changes', id: 'object-changes', data: objectChanges })
                }
                // Each subagent has one progress part in the reply, which every write replaces.
                const writeSubagentProgress = (progress: SubagentProgress) =>
                    writer.write({ type: 'data-subagent', id: String(progress.subagentId), data: progress })
                const tools = createChatToolSet({
                    systemPrompt: registry.system_prompt,
                    compactionInputTokens: registry.compaction_input_tokens,
                    rustTools: registry.tools,
                    signal: controller.signal,
                    modelRoute,
                    modelCalls,
                    profileId,
                    writeObjectChanges,
                    writeSubagentProgress,
                })
                await streamCompactingReply({
                    writer,
                    call: {
                        model: modelCalls.model,
                        providerOptions: modelRoute.providerOptions,
                        system: registry.system_prompt,
                        tools,
                        onLanguageModelCallEnd: modelCalls.onLanguageModelCallEnd,
                    },
                    compactionInputTokens: registry.compaction_input_tokens,
                    signal: controller.signal,
                    messages: await toModelInput(messages, {
                        tools,
                        ignoreIncompleteToolCalls: true,
                        convertDataPart,
                    }),
                    // Bedrock delivers text in bursts; emit complete words between those arrivals.
                    transform: smoothStream(),
                    // A failed read also stops the reply, but only a real 0 says why.
                    shouldStop: async () => {
                        const hasCredits = await hasEnoughCredits(profileId).catch(() => undefined)
                        isOutOfCredits = hasCredits === false
                        return hasCredits !== true
                    },
                    uiMessageStreamOptions: {
                        originalMessages: messages,
                        generateMessageId: () => responseId,
                        sendReasoning: true,
                        onError: (error) => {
                            console.error('Editor chat error', error)
                            return getErrorReason(error)
                        },
                        // The reply carries when it started, so a reload or a second viewer counts its time from there.
                        // Only the first chunk carries it; any other chunk with metadata would send it again.
                        messageMetadata: ({ part }) =>
                            part.type === 'start' ? { startedAtMs } : undefined,
                    },
                })
            },
        })
        isStreamStarted = true
        // Consume independently so closing a tab does not discard completed tools or the reply.
        const persistResponse = async () => {
            let lastPublishedAt = 0
            let savedSteps = 0
            // The reply is saved when each step starts, after the previous step's tools have finished.
            // A failed save is logged and the next one catches up, so the database never stops the reply.
            let saving = Promise.resolve()
            let latest: UIMessage | undefined
            // An update inside the 50 ms is sent when it ends, so a tool that starts just after
            // a publish still shows as running while the stream is quiet.
            let pendingPublish: NodeJS.Timeout | undefined
            const publishReply = () => {
                if (!latest) return
                // The chat never shows a tool's input, so browsers get it once the AI has finished writing it.
                const reply: UIMessage = {
                    ...latest,
                    parts: latest.parts.map((part) =>
                        isToolUIPart(part) && part.state === 'input-streaming'
                            ? { ...part, input: undefined }
                            : part,
                    ),
                }
                const update = getMessageUpdate(turn.publishedReply, reply)
                if (!update) return
                // Each message the stream gives is a fresh copy that never changes, so it is kept as it is.
                turn.publishedReply = reply
                chatEvents.publish(update)
            }
            try {
                for await (const message of readUIMessageStream({
                    stream,
                    terminateOnError: true,
                })) {
                    latest = message
                    // Nothing else arrives while a summary is written, so a compaction is saved at once.
                    if (
                        countSteps(message) !== savedSteps ||
                        message.parts.at(-1)?.type === 'data-compaction'
                    ) {
                        savedSteps = countSteps(message)
                        saving = saving
                            .then(() => saveMessage({ ...message, metadata: { startedAtMs, isRunning: true } }))
                            .catch((error) => console.error('Could not save the reply', error))
                    }
                    // TODO: AI-PICKED-VALUE: Browsers get the reply at most every 50 ms, about five words of smoothStream's output.
                    if (Date.now() - lastPublishedAt > 50) {
                        clearTimeout(pendingPublish)
                        pendingPublish = undefined
                        publishReply()
                        lastPublishedAt = Date.now()
                    } else
                        pendingPublish ??= setTimeout(() => {
                            pendingPublish = undefined
                            publishReply()
                        }, 50)
                }
            } catch (error) {
                isInterrupted = true
                failureReason ??= getErrorReason(error)
                controller.abort()
                console.error('Editor chat interrupted', error)
            } finally {
                clearTimeout(pendingPublish)
                // From here on whole copies of the reply are published, and a browser that connects reads the saved one.
                turn.publishedReply = undefined
                void modelCalls.chargeStoppedCalls()
                await saving
                try {
                    const isFailed = (isInterrupted || controller.signal.aborted) && !turn.isStopRequested
                    const reason = failureReason ?? 'The reply stopped unexpectedly.'
                    // A reply that failed before showing anything is removed, so the user's message
                    // is the last one again and Retry resumes it the way a homepage prompt resumes.
                    if (isFailed && !hasVisibleOutput(latest)) {
                        await database.deleteChatMessage(responseId)
                        latest = undefined
                    } else if (latest) {
                        if (isFailed)
                            latest = { ...latest, parts: [...latest.parts, { type: 'data-reply-failed', data: { reason } }] }
                        await saveMessage(latest)
                        chatEvents.publish({ type: 'message', message: latest })
                    }
                    if (isFailed)
                        chatEvents.publish({
                            type: 'reply-failed',
                            reason,
                            messageId: userMessage.id,
                            isModelOverloaded: reason === MODEL_OVERLOADED_REASON,
                        })
                    else if (isInterrupted || controller.signal.aborted)
                        chatEvents.publish({ type: 'interrupted' })
                    else if (isOutOfCredits) chatEvents.publish({ type: 'out-of-credits' })
                } finally {
                    // The objects the turn changed are saved under its reply. The model never sees them.
                    // Without the final list the reply keeps the one it built while it ran.
                    const objectChanges = await reportChatFinished()
                    const earlierParts = latest?.parts.filter((part) => part.type !== 'data-object-changes') ?? []
                    if (latest && objectChanges && (objectChanges.length > 0 || earlierParts.length !== latest.parts.length)) {
                        const message: UIMessage = {
                            ...latest,
                            parts: [
                                ...earlierParts,
                                ...(objectChanges.length > 0
                                    ? [{ type: 'data-object-changes' as const, id: 'object-changes', data: objectChanges }]
                                    : []),
                            ],
                        }
                        await saveMessage(message).then(
                            () => chatEvents.publish({ type: 'message', message }),
                            (error) => console.error('Could not save the turn changes', error),
                        )
                    }
                    finishTurn()
                }
            }
        }
        void persistResponse().catch((error) =>
            console.error('Could not persist editor chat', error),
        )
        // Generation is consumed by the server; every browser receives the same chat events.
        return new Response(null, { status: 204 })
    } finally {
        if (!isStreamStarted) {
            if (isReplySaved)
                await database
                    .deleteChatMessage(responseId)
                    .catch((error) => console.error('Could not remove the unstarted reply', error))
            await reportChatFinished()
            finishTurn()
        }
    }
}
