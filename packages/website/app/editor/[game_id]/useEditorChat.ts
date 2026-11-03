import { useCallback, useEffect, useRef, useState } from 'react'
import { validateUIMessages, type UIMessage } from 'ai'
import { z } from 'zod'
import { toast } from 'sonner'
import posthog from 'posthog-js'
import { editorRequest, isOutOfCreditsError } from '../editorApi'
import { readEditorEvents } from '../editorEvents'
import { applyMessageUpdate } from '../editorChat'
import type { EditorModelId } from './EditorModelPicker'

const chatEventSchema = z.discriminatedUnion('type', [
    z.object({ type: z.literal('state'), messages: z.array(z.unknown()) }),
    z.object({ type: z.literal('message'), message: z.unknown() }),
    z.object({ type: z.literal('message-update'), messageId: z.string(), metadata: z.unknown(), partCount: z.int().nonnegative(), changedParts: z.array(z.object({ index: z.int().nonnegative(), part: z.unknown() })), addedTexts: z.array(z.object({ index: z.int().nonnegative(), text: z.string() })) }),
    z.object({ type: z.literal('interrupted') }),
    z.object({ type: z.literal('out-of-credits') }),
    z.object({ type: z.literal('reply-failed'), reason: z.string(), messageId: z.string(), isModelOverloaded: z.boolean().optional() }),
])

const hasVisibleOutput = (message: UIMessage) => message.parts.some(part => (part.type === 'text' && part.text.trim() !== '') || part.type === 'dynamic-tool' || part.type.startsWith('tool-'))

// AI SDK assembles messages on Node. Each browser gets one snapshot followed by the same live updates.
export const useEditorChat = (inviteToken: string | null, chatUrl: string, isChatRunning: boolean, model: EditorModelId, restoreInput: (text: string) => void) => {
    const [messages, setMessages] = useState<UIMessage[]>([])
    const [isSending, setIsSending] = useState(false)
    const [isChatConnected, setIsChatConnected] = useState(false)
    const [isOutOfCredits, setIsOutOfCredits] = useState(false)
    // Shown as soon as it is sent, until the saved copy with the same id replaces it.
    const [pendingMessage, setPendingMessage] = useState<UIMessage>()
    const [failure, setFailure] = useState<{ messageId: string, reason: string }>()
    const initialMessageRef = useRef<UIMessage | undefined>(undefined)
    const savedMessageIdsRef = useRef(new Set<string>())
    savedMessageIdsRef.current = new Set(messages.map(message => message.id))
    const sendMessage = useCallback(async (message: UIMessage) => {
        setPendingMessage(message)
        setFailure(undefined)
        setIsSending(true)
        try { await editorRequest(inviteToken, chatUrl, '/chat-message', 'POST', { message, model }, undefined); setIsOutOfCredits(false); posthog.capture('editor-chat-submit') }
        catch (error) {
            // A message the server never saved goes back in the box, whatever stopped it. A saved one, like the
            // homepage prompt, stays in the chat with Retry.
            setIsOutOfCredits(isOutOfCreditsError(error))
            setPendingMessage(undefined)
            if (savedMessageIdsRef.current.has(message.id)) setFailure({ messageId: message.id, reason: '' })
            else restoreInput(message.parts.flatMap(part => part.type === 'text' ? [part.text] : []).join('\n'))
        }
        finally { setIsSending(false) }
    }, [chatUrl, inviteToken, model, restoreInput])
    useEffect(() => {
        const controller = new AbortController()
        const listen = async () => {
            let isInitialConnection = true
            // Each completed turn closes the response. Reopening it checks access and reloads the saved snapshot.
            // A response that breaks, like one the server dropped for falling behind, is reopened the same way.
            while (!controller.signal.aborted) {
                try {
                    const response = await editorRequest(inviteToken, chatUrl, '/editor-server-chat-stream', 'GET', undefined, controller.signal)
                    for await (const value of readEditorEvents(response)) {
                        const event = chatEventSchema.parse(value)
                        if (event.type === 'state') {
                            const initialMessages = event.messages.length === 0 ? [] : await validateUIMessages({ messages: event.messages })
                            // Only the first connection can resume the homepage prompt; reconnection must never resend it.
                            if (isInitialConnection) initialMessageRef.current = initialMessages.at(-1)
                            isInitialConnection = false
                            setMessages(initialMessages)
                            setIsChatConnected(true)
                        } else if (event.type === 'message') {
                            const [message] = await validateUIMessages({ messages: [event.message] })
                            setMessages(previous => {
                                const index = previous.findIndex(item => item.id === message.id)
                                return index === -1 ? [...previous, message] : previous.with(index, message)
                            })
                        } else if (event.type === 'message-update') {
                            // The changed parts are checked the way a whole message is, as one message holding only them.
                            const [{ parts }] = await validateUIMessages({ messages: [{ id: event.messageId, role: 'assistant', parts: event.changedParts.map(({ part }) => part) }] })
                            const changedParts = parts.map((part, index) => ({ index: event.changedParts[index].index, part }))
                            setMessages(previous => applyMessageUpdate(previous, { ...event, changedParts }))
                        } else if (event.type === 'out-of-credits') setIsOutOfCredits(true)
                        else if (event.type === 'reply-failed') {
                            // A reply that failed before showing anything was deleted, so the message is the last one again.
                            setMessages(previous => previous.at(-1)?.role === 'assistant' && !hasVisibleOutput(previous.at(-1)!) ? previous.slice(0, -1) : previous)
                            setFailure({ messageId: event.messageId, reason: event.reason })
                            if (event.isModelOverloaded) toast(`Sorry, we're having trouble getting responses from that model. Please try in a couple minutes, or change models.`)
                        }
                    }
                    setIsChatConnected(false)
                } catch (error) {
                    if (controller.signal.aborted) return
                    console.error('Editor chat disconnected', error)
                    setIsChatConnected(false)
                    toast.error('Chat disconnected, reconnecting…', { id: 'editor-chat-connection' })
                    // TODO: AI-PICKED-VALUE: Wait two seconds before reopening a broken chat stream, like the editor's own start polling.
                    await new Promise<void>(resolve => setTimeout(resolve, 2000))
                }
            }
        }
        void listen()
        return () => controller.abort()
    }, [chatUrl, inviteToken])
    useEffect(() => {
        if (!isChatConnected) return
        const pending = initialMessageRef.current
        initialMessageRef.current = undefined
        // Only a homepage prompt, the only message, is sent on its own. A later unanswered message waits for Retry.
        if (pending?.role !== 'user' || messages.length !== 1 || messages[0].id !== pending.id || isChatRunning) return
        void sendMessage(pending)
    }, [isChatRunning, messages, sendMessage, isChatConnected])
    const shownMessages = pendingMessage && !messages.some(message => message.id === pendingMessage.id) ? [...messages, pendingMessage] : messages
    const isResponding = isSending || isChatRunning
    const lastMessage = shownMessages.at(-1)
    const unsentReason = failure?.messageId === lastMessage?.id ? failure?.reason : undefined
    const unsentMessage = !isResponding && lastMessage?.role === 'user' && (unsentReason !== undefined || shownMessages.length > 1) ? lastMessage : undefined
    return { messages: shownMessages, sendMessage, isResponding, isChatConnected, isOutOfCredits, unsent: unsentMessage && { message: unsentMessage, reason: unsentReason } }
}
