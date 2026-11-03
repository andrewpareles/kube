import { test } from 'node:test'
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { findFreePort, startDatabaseForTests, startPostgrestForTests } from '../../../../tests/databaseForTests.ts'
import { repoPath } from '../../../../tests/testPaths.ts'

const GAME_ID = 42

// Opt in with DATABASE_TESTS=1, Postgres 17 first on PATH and PostgREST. The worker's model call accounting runs on a throwaway
// copy of production's schema.
test('model calls are charged once each, in full, and none are lost when they finish together', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
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
    const { CREDITS_PER_DAY, hasEnoughCredits, recordModelCall } = await import('../src/helpers/chatUsage.ts')
    const { getModelRoute } = await import('../src/helpers/model.ts')
    const modelRoute = getModelRoute('openai.gpt-6-astra')
    try {
        let profileCount = 0
        const addProfile = async () => {
            profileCount += 1
            const [profile] = await sql`insert into user_profiles (id, username) values (${randomUUID()}::uuid, ${`player${profileCount}`}) returning id`
            return String(profile.id)
        }
        await sql`insert into game_metadata (game_id, name, creator_id, max_players) values (${GAME_ID}, 'Game', ${await addProfile()}::uuid, 8)`
        const tokens = { noCache: 1000, cacheRead: 2000, cacheWrite: 300, output: 400 }
        const record = (profileId: string, responseId: string, credits: number) => recordModelCall({ modelRoute, profileId, messageId: 'message-1', responseId, tokens, isOutputKnown: true, credits })
        const readCreditsUsedToday = async (profileId: string) => Number((await sql`select credits_used_today(${profileId}::uuid) as credits`)[0].credits)
        const readUsage = (profileId: string) => sql`select message_id, game_id, model_id, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, aws_request_id, credits_charged from chat_usage where profile_id = ${profileId}::uuid order by id`

        await t.test('a call recorded twice is charged once, with its tokens and game', async () => {
            const profileId = await addProfile()
            await record(profileId, 'response-1', 10)
            await record(profileId, 'response-1', 10)
            assert.equal(await readCreditsUsedToday(profileId), 10)
            assert.deepEqual((await readUsage(profileId)).map((row: object) => ({ ...row })), [{
                message_id: 'message-1', game_id: String(GAME_ID), model_id: 'openai.gpt-6-astra', input_tokens: 1000, cache_read_tokens: 2000,
                cache_write_tokens: 300, output_tokens: 400, aws_request_id: 'response-1', credits_charged: 10,
            }])
        })

        await t.test('a profile can use the AI until what it used today reaches CREDITS_PER_DAY', async () => {
            const profileId = await addProfile()
            await record(profileId, 'response-almost-all', CREDITS_PER_DAY - 1)
            assert.equal(await hasEnoughCredits(profileId), true)
            await record(profileId, 'response-the-rest', 1)
            assert.equal(await hasEnoughCredits(profileId), false)
        })

        await t.test('calls that finish together are all charged', async () => {
            const profileId = await addProfile()
            await Promise.all(Array.from({ length: 10 }, (_, index) => record(profileId, `response-together-${index}`, 3)))
            assert.equal(await readCreditsUsedToday(profileId), 30)
            assert.equal((await readUsage(profileId)).length, 10)
        })

        await t.test('a call whose output was never reported stores no output tokens', async () => {
            const profileId = await addProfile()
            await recordModelCall({ modelRoute, profileId, messageId: null, responseId: 'response-no-output', tokens, isOutputKnown: false, credits: 1 })
            const [usage] = await readUsage(profileId)
            assert.equal(usage.output_tokens, null)
            assert.equal(usage.message_id, null)
        })
    } finally {
        await sql.end()
        await postgrest.stop()
        await database.stop()
    }
})
