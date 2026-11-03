import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'

// Run explicitly with npm run test:thumbnail. It calls OpenAI's model on Azure with the key in packages/website/.env.local, so each run
// costs about a quarter. Nothing touches the database or storage.
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
})
const { generateAiThumbnail } = await import('../src/helpers/aiThumbnail.ts')

test('a publish screenshot comes back as a 16:9 PNG thumbnail', async () => {
    const screenshot = await readFile(
        new URL('../../../../packages/website/public/site-fold/282.png', import.meta.url),
    )
    const png = await generateAiThumbnail({
        name: 'Snowball Village Showdown',
        description: null,
        screenshot_png_base64: screenshot.toString('base64'),
    })
    assert.equal(png.subarray(1, 4).toString('latin1'), 'PNG')
    assert.deepEqual([png.readUInt32BE(16), png.readUInt32BE(20)], [2048, 1152])
})
