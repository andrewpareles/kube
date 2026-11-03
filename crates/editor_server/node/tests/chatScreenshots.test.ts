import { test } from 'node:test'
import assert from 'node:assert/strict'
import { GetObjectCommand, PutObjectCommand, S3Client } from '@aws-sdk/client-s3'

Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'this-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: '8080',
    EDITOR_CHAT_PORT: '8081',
    SITE_URL: 'https://website.example',
    SUPABASE_URL: 'https://supabase.invalid',
    SUPABASE_SECRET_KEY: 'test-only',
    S3_ACCESS_KEY_ID: 'test-only',
    S3_SECRET_ACCESS_KEY: 'test-only',
    AWS_REGION: 'us-east-1',
    AWS_BEARER_TOKEN_BEDROCK: 'test-only',
    AZURE_OPENAI_API_KEY: 'test-only',
})
const { createChatScreenshots } = await import('../src/helpers/chatScreenshots.ts')

type StoredFile = { bytes: Buffer; contentType: string | undefined }
const withFakeS3 = async (
    run: (stored: Map<string, StoredFile>, reads: string[]) => Promise<void>,
) => {
    const stored = new Map<string, StoredFile>()
    const reads: string[] = []
    const originalSend = S3Client.prototype.send
    S3Client.prototype.send = (async (command: unknown) => {
        if (command instanceof PutObjectCommand) {
            const { Bucket, Key, Body, ContentType } = command.input
            stored.set(`${Bucket}/${Key}`, {
                bytes: Buffer.from(Body as Buffer),
                contentType: ContentType,
            })
            return {}
        }
        if (command instanceof GetObjectCommand) {
            const path = `${command.input.Bucket}/${command.input.Key}`
            reads.push(path)
            const file = stored.get(path)
            if (!file) throw new Error(`NoSuchKey ${path}`)
            return {
                Body: {
                    transformToString: async (encoding: BufferEncoding) =>
                        file.bytes.toString(encoding),
                },
                ContentType: file.contentType,
            }
        }
        throw new Error('Unexpected S3 command')
    }) as unknown as typeof S3Client.prototype.send
    try {
        await run(stored, reads)
    } finally {
        S3Client.prototype.send = originalSend
    }
}
const jpeg = {
    media_type: 'image/jpeg',
    base64: Buffer.from('not really a jpeg').toString('base64'),
}

test('a screenshot is uploaded under its game and read back from memory in the same reply', async () => {
    await withFakeS3(async (stored, reads) => {
        const screenshots = createChatScreenshots()
        const screenshotUuid = screenshots.add(jpeg)
        assert.match(screenshotUuid, /^[0-9a-f-]{36}$/)
        assert.deepEqual(await screenshots.read(screenshotUuid), jpeg)
        await new Promise((resolve) => setImmediate(resolve))
        const file = stored.get(`kube-new-private/chat-screenshots/42/${screenshotUuid}`)
        assert.equal(file?.bytes.toString('base64'), jpeg.base64)
        assert.equal(file?.contentType, 'image/jpeg')
        assert.equal(reads.length, 0)
    })
})

test('a later reply downloads a screenshot once, with its type, and a missing one reads as undefined', async () => {
    await withFakeS3(async (_stored, reads) => {
        const screenshotUuid = createChatScreenshots().add(jpeg)
        await new Promise((resolve) => setImmediate(resolve))
        const laterReply = createChatScreenshots()
        assert.deepEqual(await laterReply.read(screenshotUuid), jpeg)
        assert.deepEqual(await laterReply.read(screenshotUuid), jpeg)
        assert.equal(reads.length, 1)
        assert.equal(await laterReply.read('00000000-0000-0000-0000-000000000000'), undefined)
    })
})
