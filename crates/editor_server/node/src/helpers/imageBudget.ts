import type { ModelMessage, ToolResultPart } from 'ai'

// TODO: AI-PICKED-VALUE: A request carries at most 16 MB of images, under the 20 MB Bedrock documents for a whole request
// (our route accepted 32 MB when tested).
const IMAGE_BUDGET_BYTES = 16 * 1024 * 1024
// TODO: AI-PICKED-VALUE: Images go 8 MB at a time, about 100 screenshots, so the prompt cache is rebuilt once per batch.
const IMAGE_BATCH_BYTES = 8 * 1024 * 1024
// TODO: AI-PICKED-VALUE: Counting every image as at least 32 KB also keeps a request under Claude's 600-image limit.
const MIN_IMAGE_BYTES = 32 * 1024
const REMOVED_IMAGE_NOTE =
    'An older image was removed here to keep the conversation small. Capture or view it again if you need it.'

type ToolResultContent = Extract<ToolResultPart['output'], { type: 'content' }>['value'][number]

// Images only reach the model inside tool results. Each one is passed to `map` with its size as sent, in order.
const mapImages = (
    messages: ModelMessage[],
    map: (part: ToolResultContent, imageBytes: number) => ToolResultContent,
): ModelMessage[] =>
    messages.map((message) =>
        message.role !== 'tool'
            ? message
            : {
                  ...message,
                  content: message.content.map((part) =>
                      part.type !== 'tool-result' || part.output.type !== 'content'
                          ? part
                          : {
                                ...part,
                                output: {
                                    ...part.output,
                                    value: part.output.value.map((item) =>
                                        item.type !== 'file' ||
                                        !item.mediaType.startsWith('image') ||
                                        item.data.type !== 'data'
                                            ? item
                                            : map(
                                                  item,
                                                  typeof item.data.data === 'string'
                                                      ? item.data.data.length
                                                      : Math.ceil(
                                                            (item.data.data.byteLength * 4) / 3,
                                                        ),
                                              ),
                                    ),
                                },
                            },
                  ),
              },
    )

// Like Claude Code, the oldest images are dropped in whole batches once the images pass the budget, so the cached prompt
// changes once per batch. What to drop is worked out from the whole history each time, so every step, the next reply and
// the summary drop the same images.
export const keepImagesWithinBudget = (messages: ModelMessage[]) => {
    let totalBytes = 0
    mapImages(messages, (part, bytes) => {
        totalBytes += Math.max(bytes, MIN_IMAGE_BYTES)
        return part
    })
    if (totalBytes <= IMAGE_BUDGET_BYTES) return messages
    const bytesToDrop =
        Math.ceil((totalBytes - IMAGE_BUDGET_BYTES) / IMAGE_BATCH_BYTES) * IMAGE_BATCH_BYTES
    let droppedBytes = 0
    return mapImages(messages, (part, bytes) => {
        if (droppedBytes >= bytesToDrop) return part
        droppedBytes += Math.max(bytes, MIN_IMAGE_BYTES)
        return { type: 'text', text: REMOVED_IMAGE_NOTE }
    })
}
