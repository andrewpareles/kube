import { test } from 'node:test'
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import type { UIMessage } from 'ai'
import { HTTPException } from 'hono/http-exception'
import {
    findFreePort,
    startDatabaseForTests,
    startPostgrestForTests,
} from '../../../../tests/databaseForTests.ts'
import { repoPath } from '../../../../tests/testPaths.ts'

const GAME_ID = 42

// Opt in with DATABASE_TESTS=1, Postgres 17 first on PATH and PostgREST. The chat child's calls that the other database tests
// don't make go through PostgREST to a throwaway copy of production's schema.
test(
    'the chat child checks the browser, starts turns, reads the chat and checks credits through PostgREST',
    { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 },
    async (t) => {
        const testDatabase = await startDatabaseForTests(await findFreePort())
        const postgrest = await startPostgrestForTests(testDatabase.url)
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
        const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(
            testDatabase.url,
            { onnotice: () => {} },
        )
        const { database } = await import('../src/helpers/database.ts')
        try {
            let profileCount = 0
            const addProfile = async () => {
                profileCount += 1
                const [profile] =
                    await sql`insert into user_profiles (id, username) values (${randomUUID()}::uuid, ${`player${profileCount}`}) returning id`
                return String(profile.id)
            }
            const creatorId = await addProfile()
            await sql`insert into game_metadata (game_id, name, creator_id, max_players) values (${GAME_ID}, 'Game', ${creatorId}::uuid, 8)`
            const setChat = async (messages: UIMessage[]) => {
                await sql`delete from chat_messages where game_id = ${GAME_ID}`
                for (const [index, message] of messages.entries())
                    await sql`insert into chat_messages (game_id, seq, message) values (${GAME_ID}, ${index + 1}, ${JSON.stringify(message)}::text::jsonb)`
            }
            const userMessage = (id: string): UIMessage => ({
                id,
                role: 'user',
                parts: [{ type: 'text', text: 'Build a race track' }],
            })
            const reply = (id: string): UIMessage => ({
                id,
                role: 'assistant',
                parts: [],
                metadata: { startedAtMs: 1_000, isRunning: true },
            })

            await t.test(
                "the creator or the game's invite gets in once this editor runs, and anyone else is refused with the function's status",
                async () => {
                    const editorStopped = {
                        status: 410,
                        message: 'Editor server stopped. Reopen the editor.',
                    }
                    await assert.rejects(database.browserAccess(creatorId, null), editorStopped)
                    await sql`insert into running_editor_servers (request_id, game_id, editor_url, public_ip, playtest_external_port) values ('this-deployment', ${GAME_ID}, 'https://editor.example', '127.0.0.1', 30000)`
                    assert.equal(await database.browserAccess(creatorId, null), creatorId)
                    await sql`insert into editor_share_auth (game_id, invite_token) values (${GAME_ID}, 'invite-1')`
                    assert.equal(await database.browserAccess(null, 'invite-1'), creatorId)
                    const refused = {
                        status: 403,
                        message:
                            'You do not have access to this editor. The invite link may have been revoked.',
                    }
                    await assert.rejects(database.browserAccess(null, 'invite-2'), refused)
                    await assert.rejects(database.browserAccess(await addProfile(), null), refused)
                },
            )

            await t.test(
                "a database error that isn't meant for the user keeps its status to itself",
                async () => {
                    await assert.rejects(
                        database.browserAccess('not-a-uuid', null),
                        (error) =>
                            !(error instanceof HTTPException) &&
                            error instanceof Error &&
                            error.message.startsWith('Database request failed (status 400): '),
                    )
                },
            )

            await t.test(
                'a turn saves its user message and reply after the history, and a resumed turn only its reply',
                async () => {
                    await setChat([userMessage('user-1')])
                    await database.startChatReply(null, reply('reply-1'))
                    await database.startChatReply(userMessage('user-2'), reply('reply-2'))
                    assert.deepEqual(await database.chatMessages(), [
                        userMessage('user-1'),
                        reply('reply-1'),
                        userMessage('user-2'),
                        reply('reply-2'),
                    ])
                },
            )

            await t.test('the history for the model starts at the latest summary', async () => {
                const summary = (id: string): UIMessage => ({
                    id,
                    role: 'assistant',
                    parts: [{ type: 'data-compaction', data: { summary: `Summary ${id}` } }],
                })
                await setChat([userMessage('user-1')])
                assert.deepEqual(await database.chatMessagesSinceSummary(), [userMessage('user-1')])
                await setChat([
                    userMessage('user-1'),
                    summary('summary-1'),
                    userMessage('user-2'),
                    summary('summary-2'),
                    userMessage('user-3'),
                ])
                assert.deepEqual(await database.chatMessagesSinceSummary(), [
                    summary('summary-2'),
                    userMessage('user-3'),
                ])
            })

            await t.test('a profile has used what its calls were charged today', async () => {
                const profileId = await addProfile()
                assert.equal(await database.creditsUsedToday(profileId), 0)
                await sql`insert into chat_usage (profile_id, model_id, aws_request_id, credits_charged) values (${profileId}::uuid, 'openai.gpt-6-astra', 'response-1', 12)`
                assert.equal(await database.creditsUsedToday(profileId), 12)
                assert.equal(await database.creditsUsedToday(randomUUID()), 0)
            })
        } finally {
            await sql.end()
            await postgrest.stop()
            await testDatabase.stop()
        }
    },
)
