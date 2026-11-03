import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { ModelMessage } from 'ai'
import { keepImagesWithinBudget } from '../src/helpers/imageBudget.ts'

const MB = 1024 * 1024
const repeat = <T>(count: number, value: T) => Array.from({ length: count }, () => value)

// A screenshot tool call and its result for each image size, whose image is a stand-in of that size.
const conversation = (imageSizes: number[]): ModelMessage[] =>
    imageSizes.flatMap((imageBytes, index): ModelMessage[] => [
        {
            role: 'assistant',
            content: [
                {
                    type: 'tool-call',
                    toolCallId: `call_${index}`,
                    toolName: 'capture_screenshot',
                    input: {},
                },
            ],
        },
        {
            role: 'tool',
            content: [
                {
                    type: 'tool-result',
                    toolCallId: `call_${index}`,
                    toolName: 'capture_screenshot',
                    output: {
                        type: 'content',
                        value: [
                            { type: 'text', text: `Screenshot ${index}` },
                            {
                                type: 'file',
                                mediaType: 'image/jpeg',
                                data: { type: 'data', data: 'A'.repeat(imageBytes) },
                            },
                        ],
                    },
                },
            ],
        },
    ])

// For each screenshot, oldest first, whether the model gets its image ('file') or the note in its place ('text').
const getScreenshotPartTypes = (messages: ModelMessage[]) =>
    messages.flatMap((message) =>
        message.role !== 'tool'
            ? []
            : message.content.map((part) =>
                  part.type === 'tool-result' && part.output.type === 'content'
                      ? part.output.value.at(-1)?.type
                      : undefined,
              ),
    )

test('a conversation whose images fit the budget is sent unchanged', () => {
    const messages = conversation(repeat(16, MB))
    assert.equal(keepImagesWithinBudget(messages), messages)
})

test('past 16 MB, the oldest images are replaced by a note, 8 MB at a time', () => {
    assert.deepEqual(getScreenshotPartTypes(keepImagesWithinBudget(conversation(repeat(17, MB)))), [
        ...repeat(8, 'text'),
        ...repeat(9, 'file'),
    ])
    assert.deepEqual(getScreenshotPartTypes(keepImagesWithinBudget(conversation(repeat(25, MB)))), [
        ...repeat(16, 'text'),
        ...repeat(9, 'file'),
    ])
})

// The cached prompt is the start of the request, so a new screenshot must not change the messages before it until the
// next batch is due.
test('a new image leaves the earlier messages unchanged until the next batch', () => {
    for (let imageCount = 17; imageCount < 24; imageCount++) {
        const sent = keepImagesWithinBudget(conversation(repeat(imageCount, MB)))
        const sentAfterOneMore = keepImagesWithinBudget(conversation(repeat(imageCount + 1, MB)))
        assert.deepEqual(sentAfterOneMore.slice(0, sent.length), sent, `${imageCount} images`)
    }
})

test('every image counts as at least 32 KB, so many small images stay under the 600-image limit', () => {
    const partTypes = getScreenshotPartTypes(
        keepImagesWithinBudget(conversation(repeat(700, 1024))),
    )
    assert.equal(partTypes.filter((partType) => partType === 'file').length, 444)
})
