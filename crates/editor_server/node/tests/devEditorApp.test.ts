import { test } from 'node:test'
import assert from 'node:assert/strict'

process.argv.push('--dev')
Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'current-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: '8080',
    EDITOR_CHAT_PORT: '8081',
    SITE_URL: 'http://localhost:3000',
    SUPABASE_URL: 'https://supabase.invalid',
    SUPABASE_SECRET_KEY: 'test-only',
    S3_ACCESS_KEY_ID: 'test-only',
    S3_SECRET_ACCESS_KEY: 'test-only',
    AWS_REGION: 'us-east-1',
    AWS_BEARER_TOKEN_BEDROCK: 'test-only',
    AZURE_OPENAI_API_KEY: 'test-only',
})
const { app } = await import('../src/endpoints.ts')
test('dev chat URLs cannot reach a different game after the launcher replaces its editor', async () => {
    for (const [method, path] of [
        ['GET', '/editor-server-chat-stream'],
        ['POST', '/chat-message'],
        ['POST', '/chat-interrupt'],
        ['POST', '/script-diagnostics'],
        ['GET', '/get-script-types'],
    ]) {
        assert.equal((await app.request(`/old-deployment${path}`, { method })).status, 404)
        assert.equal((await app.request(path, { method })).status, 404)
        assert.equal((await app.request(`/current-deployment${path}`, { method })).status, 401)
    }
})
