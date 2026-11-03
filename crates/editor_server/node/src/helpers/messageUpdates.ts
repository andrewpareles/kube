import { isDeepStrictEqual } from 'node:util'
import type { UIMessage } from 'ai'

type MessageUpdate =
    | { type: 'message'; message: UIMessage }
    | {
          type: 'message-update'
          messageId: string
          metadata: unknown
          partCount: number
          changedParts: { index: number; part: UIMessage['parts'][number] }[]
          addedTexts: { index: number; text: string }[]
      }

// While a reply streams, every browser holds it as it was last published. An update carries only the parts that changed
// since then, each with its position, so text, tool calls and results that were already sent are not sent again. Parts
// never move while a reply streams: new ones are added at the end, and one that changes, like the list of changed objects
// near the top, keeps its place. The first publish of a reply is the whole message.
// A text or reasoning part whose text only grew carries just the text added to its end.
export const getMessageUpdate = (
    published: UIMessage | undefined,
    latest: UIMessage,
): MessageUpdate | undefined => {
    if (!published) return { type: 'message', message: latest }
    const changedParts: { index: number; part: UIMessage['parts'][number] }[] = []
    const addedTexts: { index: number; text: string }[] = []
    for (const [index, part] of latest.parts.entries()) {
        const publishedPart = published.parts[index]
        if (isDeepStrictEqual(part, publishedPart)) continue
        if (
            (part.type === 'text' || part.type === 'reasoning') &&
            (publishedPart?.type === 'text' || publishedPart?.type === 'reasoning') &&
            part.text.startsWith(publishedPart.text) &&
            isDeepStrictEqual({ ...publishedPart, text: part.text }, part)
        )
            addedTexts.push({ index, text: part.text.slice(publishedPart.text.length) })
        else changedParts.push({ index, part })
    }
    if (
        changedParts.length === 0 &&
        addedTexts.length === 0 &&
        isDeepStrictEqual(published.metadata, latest.metadata)
    )
        return undefined
    return {
        type: 'message-update',
        messageId: latest.id,
        metadata: latest.metadata,
        partCount: latest.parts.length,
        changedParts,
        addedTexts,
    }
}
