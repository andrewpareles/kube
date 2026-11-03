import type { UIMessage } from 'ai'
import { getObjectKind, type LiveObject } from './editorObjectSchema.ts'

// Snapshot the complete selection into the message so later selection changes cannot change its context.
export const createEditorChatMessage = (text: string, objects: LiveObject[]): UIMessage => ({
    id: crypto.randomUUID(), role: 'user', parts: [
        { type: 'text', text },
        ...objects.length === 0 ? [] : [{ type: 'data-selected-objects', data: objects.map(object => ({ objectId: object.id, name: object.name, kind: getObjectKind(object) })) } satisfies UIMessage['parts'][number]],
    ],
})

// While a reply streams, an update lists only the parts that changed, at their positions; every other part is the one this
// browser already holds. A browser always holds the reply an update applies to, because one that connects mid-reply starts
// from the reply as it was last published.
export const applyMessageUpdate = (messages: UIMessage[], { messageId, metadata, partCount, changedParts, addedTexts }: { messageId: string, metadata: unknown, partCount: number, changedParts: { index: number, part: UIMessage['parts'][number] }[], addedTexts: { index: number, text: string }[] }) => {
    const messageIndex = messages.findIndex(message => message.id === messageId)
    if (messageIndex === -1) return messages
    const partOfIndex: { [index: number]: UIMessage['parts'][number] } = Object.fromEntries(changedParts.map(({ index, part }) => [index, part]))
    const message = messages[messageIndex]
    const parts = Array.from({ length: partCount }, (_, index) => partOfIndex[index] ?? message.parts[index])
    for (const { index, text } of addedTexts) {
        const part = parts[index]
        if (part.type === 'text' || part.type === 'reasoning') parts[index] = { ...part, text: part.text + text }
    }
    return messages.with(messageIndex, { ...message, metadata, parts })
}
