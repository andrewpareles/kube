import { test } from 'node:test'
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { findFreePort, startDatabaseForTests, startPostgrestForTests } from '../../../../tests/databaseForTests.ts'
import { repoPath } from '../../../../tests/testPaths.ts'

const GAME_ID = 42
const OTHER_GAME_ID = 43

// Opt in with DATABASE_TESTS=1, Postgres 17 first on PATH and PostgREST. What a starting editor does with replies an editor
// before it left running runs on a throwaway copy of production's schema.
test('a reply left running by an editor that died is failed, or removed if it showed nothing', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const postgrest = await startPostgrestForTests(database.url)
    Object.assign(process.env, {
        EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
        ARBITRIUM_REQUEST_ID: 'this-deployment',
        GAME_ID: String(GAME_ID),
        EDITOR_SERVER_PORT: '8080',
        EDITOR_CHAT_PORT: '8081',
        SITE_URL: 'https://website.example',
        SUPABASE_URL: postgrest.url,
        SUPABASE_SECRET_KEY: postgrest.serviceKey,
        S3_ACCESS_KEY_ID: 'test-only',
        S3_SECRET_ACCESS_KEY: 'test-only',
        AWS_REGION: 'us-east-1',
        AWS_BEARER_TOKEN_BEDROCK: 'test-only',
        AZURE_OPENAI_API_KEY: 'test-only',
    })
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    const { failRepliesLeftRunning } = await import('../src/helpers/chat.ts')
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        for (const gameId of [GAME_ID, OTHER_GAME_ID])
            await sql`insert into game_metadata (game_id, name, creator_id, max_players) values (${gameId}, 'Game', ${creatorId}::uuid, 8)`
        const userMessage = { id: 'user-message', role: 'user', parts: [{ type: 'text', text: 'Build a race track' }] }
        const runningMetadata = { startedAtMs: 1_000, isRunning: true }
        const replyWithText = { id: 'reply', role: 'assistant', parts: [{ type: 'step-start' }, { type: 'text', text: 'Placing the track', state: 'done' }] }
        const replyWithNothingShown = { id: 'reply', role: 'assistant', parts: [{ type: 'step-start' }, { type: 'reasoning', text: '', state: 'streaming' }] }
        const setChat = async (gameId: number, messages: object[]) => {
            await sql`delete from chat_messages where game_id = ${gameId}`
            for (const [index, message] of messages.entries())
                await sql`insert into chat_messages (game_id, seq, message) values (${gameId}, ${index + 1}, ${JSON.stringify(message)}::text::jsonb)`
        }
        const readChat = async (gameId: number) => (await sql`select message from chat_messages where game_id = ${gameId} order by seq`).map((row: { message: unknown }) => row.message)

        await t.test('a reply that showed something is kept and marked failed', async () => {
            await setChat(GAME_ID, [userMessage, { ...replyWithText, metadata: runningMetadata }])
            await failRepliesLeftRunning()
            assert.deepEqual(await readChat(GAME_ID), [userMessage, {
                ...replyWithText,
                parts: [...replyWithText.parts, { type: 'data-reply-failed', data: { reason: 'The editor stopped before this reply finished.' } }],
            }])
        })

        await t.test('a reply that showed nothing is removed, so the user message is last for Retry', async () => {
            await setChat(GAME_ID, [userMessage, { ...replyWithNothingShown, metadata: runningMetadata }])
            await failRepliesLeftRunning()
            assert.deepEqual(await readChat(GAME_ID), [userMessage])
        })

        await t.test('a reply that finished is left alone', async () => {
            const finishedReply = { ...replyWithNothingShown, metadata: { startedAtMs: 1_000 } }
            await setChat(GAME_ID, [userMessage, finishedReply])
            await failRepliesLeftRunning()
            assert.deepEqual(await readChat(GAME_ID), [userMessage, finishedReply])
        })

        await t.test("another game's running reply is left to its own editor", async () => {
            const otherReply = { ...replyWithText, metadata: runningMetadata }
            await setChat(OTHER_GAME_ID, [userMessage, otherReply])
            await failRepliesLeftRunning()
            assert.deepEqual(await readChat(OTHER_GAME_ID), [userMessage, otherReply])
        })
    } finally {
        await sql.end()
        await postgrest.stop()
        await database.stop()
    }
})
