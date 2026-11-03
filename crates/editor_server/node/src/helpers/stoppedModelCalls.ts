import { APICallError, wrapLanguageModel, type LanguageModelCallEndEvent } from 'ai'
import type { ModelRoute } from './model.ts'
import { getCreditsOfTokens, recordChatUsage, recordModelCall } from './chatUsage.ts'

// ============================= Charging a model call that was cut off =============================
//
// Bedrock reports a call's tokens only in the last event of its stream. When a run stops mid-call (the
// Stop button, a timeout, a failed save), that event never arrives and onLanguageModelCallEnd never
// fires. Each route says what such a call is charged (see models/). None ever overcharges.
//
// =================================================================================================

const chargeStoppedCall = async (
    modelRoute: ModelRoute,
    profileId: string,
    messageId: string | null,
    responseId: string,
) => {
    const call = await modelRoute.readStoppedCall(responseId)
    if (!call) return
    await recordModelCall({
        modelRoute,
        profileId,
        messageId,
        responseId,
        tokens: call.tokens,
        isOutputKnown: call.isOutputKnown,
        credits: getCreditsOfTokens(modelRoute, call.chargedTokens),
    })
}

// TODO: AI-PICKED-VALUE: A model stream that sends nothing for 30 minutes has stalled. Five minutes cut off
// calls that were still thinking silently on long conversations under load.
const MODEL_STALL_MS = 30 * 60_000
// Node calls a transformer's cancel when its stream is cancelled, but TypeScript's DOM types don't list it yet.
declare global {
    interface Transformer<I, O> {
        cancel?: (reason: unknown) => void
    }
}

// One per chat turn, shared by its subagents. The model it hands out remembers each call once Bedrock names it,
// and forgets the call when it ends normally. Whatever is left when the run ends was cut off. Every
// call is recorded under the chat reply it was made for.
export const trackModelCalls = (
    modelRoute: ModelRoute,
    profileId: string,
    messageId: string | null,
) => {
    const unfinishedResponseIds = new Set<string>()
    const pendingCharges = new Set<Promise<void>>()
    return {
        model: wrapLanguageModel({
            model: modelRoute.model,
            middleware: {
                wrapStream: async ({ doStream }) => {
                    // Bedrock can answer 200 with a body the AI SDK can't read. The SDK only retries failed statuses, so
                    // this is marked retryable.
                    const result = await Promise.resolve(doStream()).catch((error: unknown) => {
                        if (!APICallError.isInstance(error) || (error.statusCode ?? 500) >= 300)
                            throw error
                        console.error(
                            'A model call answered with a response that could not be read',
                            error.cause,
                        )
                        throw new APICallError({
                            ...error,
                            message: error.message,
                            isRetryable: true,
                        })
                    })
                    let stallTimeout: NodeJS.Timeout | undefined
                    const restartStallTimeout = <T>(
                        controller: TransformStreamDefaultController<T>,
                    ) => {
                        clearTimeout(stallTimeout)
                        // Sent as an error part, so the AI SDK can run the step again.
                        stallTimeout = setTimeout(() => {
                            controller.enqueue({
                                type: 'error',
                                error: new Error('The model stopped responding.'),
                            } as T)
                            controller.terminate()
                        }, MODEL_STALL_MS)
                    }
                    return {
                        ...result,
                        stream: result.stream.pipeThrough(
                            new TransformStream({
                                start: restartStallTimeout,
                                transform(part, controller) {
                                    restartStallTimeout(controller)
                                    if (part.type === 'response-metadata' && part.id)
                                        unfinishedResponseIds.add(part.id)
                                    controller.enqueue(part)
                                },
                                flush: () => clearTimeout(stallTimeout),
                                // A stopped or retried call is cancelled instead of flushed. Its timer would later
                                // write into the dead stream, and that throw crashes the worker.
                                cancel: () => clearTimeout(stallTimeout),
                            }),
                        ),
                    }
                },
            },
        }),
        // The AI SDK waits for this before the next step, so the charge runs without holding up the reply.
        onLanguageModelCallEnd: ({
            usage,
            responseId,
            finishReason,
        }: LanguageModelCallEndEvent) => {
            // A failed call is never charged.
            if (finishReason === 'error') {
                unfinishedResponseIds.delete(responseId)
                return
            }
            // Forgotten only once charged, so a failed charge is retried from the record at the end.
            const charge = recordChatUsage(
                modelRoute,
                profileId,
                messageId,
                usage,
                responseId,
            ).then(
                () => void unfinishedResponseIds.delete(responseId),
                (error) => console.error(`Could not charge model call ${responseId}`, error),
            )
            pendingCharges.add(charge)
            void charge.finally(() => pendingCharges.delete(charge))
        },
        chargeStoppedCalls: async () => {
            await Promise.all(pendingCharges)
            const responseIds = [...unfinishedResponseIds]
            unfinishedResponseIds.clear()
            await Promise.all(
                responseIds.map((responseId) =>
                    chargeStoppedCall(modelRoute, profileId, messageId, responseId).catch((error) =>
                        console.error(`Could not charge stopped model call ${responseId}`, error),
                    ),
                ),
            )
        },
    }
}
