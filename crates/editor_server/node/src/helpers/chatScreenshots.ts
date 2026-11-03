import { GetObjectCommand, PutObjectCommand, S3Client } from '@aws-sdk/client-s3'
import { randomUUID } from 'node:crypto'
import { env } from './env.ts'
import type { ToolImage } from './toolProtocol.ts'

const PRIVATE_BUCKET = 'kube-new-private'
const s3 = new S3Client({
    region: 'us-east-1',
    credentials: { accessKeyId: env.S3_ACCESS_KEY_ID, secretAccessKey: env.S3_SECRET_ACCESS_KEY },
})
const keyOf = (screenshotUuid: string) => `chat-screenshots/${env.GAME_ID}/${screenshotUuid}`

const upload = (screenshotUuid: string, image: ToolImage) =>
    s3.send(
        new PutObjectCommand({
            Bucket: PRIVATE_BUCKET,
            Key: keyOf(screenshotUuid),
            Body: Buffer.from(image.base64, 'base64'),
            ContentType: image.media_type,
            CacheControl: 'public, max-age=31536000, immutable',
            IfNoneMatch: '*',
        }),
        { abortSignal: AbortSignal.timeout(30_000) },
    )
const download = async (screenshotUuid: string): Promise<ToolImage> => {
    const { Body, ContentType } = await s3.send(
        new GetObjectCommand({ Bucket: PRIVATE_BUCKET, Key: keyOf(screenshotUuid) }),
        { abortSignal: AbortSignal.timeout(30_000) },
    )
    if (!Body || !ContentType) throw new Error('Screenshot download returned no file.')
    return { media_type: ContentType, base64: await Body.transformToString('base64') }
}

export const createChatScreenshots = () => {
    const imageOfUuid = new Map<string, Promise<ToolImage | undefined>>()
    return {
        add: (image: ToolImage) => {
            const screenshotUuid = randomUUID()
            imageOfUuid.set(screenshotUuid, Promise.resolve(image))
            upload(screenshotUuid, image).catch((error) =>
                console.error(`Could not upload screenshot ${screenshotUuid}`, error),
            )
            return screenshotUuid
        },
        read: (screenshotUuid: string) => {
            let image = imageOfUuid.get(screenshotUuid)
            if (!image) {
                image = download(screenshotUuid).catch((error) => {
                    console.error(`Could not download screenshot ${screenshotUuid}`, error)
                    return undefined
                })
                imageOfUuid.set(screenshotUuid, image)
            }
            return image
        },
    }
}
