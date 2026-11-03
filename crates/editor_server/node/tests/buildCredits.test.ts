import { test } from 'node:test'
import assert from 'node:assert/strict'

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
const { getCreditsOfTokens: getCreditsOfRouteTokens } = await import('../src/helpers/chatUsage.ts')
const { getModelRoute } = await import('../src/helpers/model.ts')
const getCreditsOfTokens = (tokens: Parameters<typeof getCreditsOfRouteTokens>[1]) =>
    getCreditsOfRouteTokens(getModelRoute('openai.gpt-6-astra'), tokens)
const tokens = { noCache: 0, cacheRead: 0, cacheWrite: 0, output: 0 }

test('A call that costs an exact number of credits is not rounded up', () => {
    assert.equal(getCreditsOfTokens({ ...tokens, noCache: 20_000 }), 11)
    assert.equal(getCreditsOfTokens({ ...tokens, cacheRead: 200_000 }), 11)
    assert.equal(getCreditsOfTokens({ ...tokens, cacheWrite: 16_000 }), 11)
    assert.equal(getCreditsOfTokens({ ...tokens, output: 4_000 }), 11)
})

test('Any usage costs at least one credit, and none costs nothing', () => {
    assert.equal(getCreditsOfTokens({ ...tokens, output: 1 }), 1)
    assert.equal(getCreditsOfTokens(tokens), 0)
})

test('Past 272K input tokens a call is priced as long-context', () => {
    assert.equal(getCreditsOfTokens({ ...tokens, cacheRead: 272_000 }), 15)
    assert.equal(getCreditsOfTokens({ ...tokens, cacheRead: 272_001 }), 30)
    assert.equal(getCreditsOfTokens({ ...tokens, cacheRead: 300_000, output: 1_000 }), 38)
})

test('Opus 5.5 has one price at any input size', () => {
    const opus = getModelRoute('anthropic.claude-opus-5-5')
    assert.equal(getCreditsOfRouteTokens(opus, { ...tokens, cacheRead: 1_000_000 }), 11)
    assert.equal(getCreditsOfRouteTokens(opus, { ...tokens, noCache: 50_000, output: 10_000 }), 22)
})
