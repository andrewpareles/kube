import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The functions editors call through PostgREST run on a throwaway copy
// of production's schema with the pending migrations applied. PostgREST answers a function's 'PT<status>' error with that status,
// so these read the error's code.
test('the editor database functions check the editor, number chat turns, sell only owned meshes and charge each call once', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { max: 25, onnotice: () => {} })
    const codeOf = async (query: Promise<unknown>) => {
        try {
            await query
            return undefined
        } catch (error) {
            return (error as { code?: string }).code
        }
    }
    try {
        let profileCount = 0
        const addProfile = async () => {
            profileCount += 1
            const [profile] = await sql`insert into user_profiles (id, username) values (${randomUUID()}::uuid, ${`player${profileCount}`}) returning id`
            return String(profile.id)
        }
        const addGame = async (creatorId: string) => {
            const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
            return Number(game.game_id)
        }

        await t.test('only the server can call these functions, not the browser keys', async () => {
            const names = [
                'charge_chat_usage', 'chat_messages_since_summary', 'claim_running_player_session', 'credits_used_today', 'delete_running_player_session_unless_play_auth_changed', 'delete_running_server_if_empty',
                'editor_browser_access', 'editor_load_world', 'editor_remove_deployment', 'editor_save_world', 'publish_catalog_item', 'record_asset_import', 'search_asset_metadata', 'start_chat_reply',
            ]
            const rows = await sql`
                select proname::text as name, has_function_privilege('anon', oid, 'execute') as anon, has_function_privilege('authenticated', oid, 'execute') as authenticated, has_function_privilege('service_role', oid, 'execute') as service_role
                from pg_proc where pronamespace = 'public'::regnamespace and proname::text = any(${names}::text[]) order by proname`
            assert.deepEqual(rows.map((row: object) => ({ ...row })), names.map(name => ({ name, anon: false, authenticated: false, service_role: true })))
        })

        await t.test('a function added later is only the server\'s until it is granted to the browser keys', async () => {
            await sql`create function public.added_later() returns integer language sql as 'select 1'`
            const [privileges] = await sql`
                select has_function_privilege('anon', 'public.added_later()', 'execute') as anon, has_function_privilege('authenticated', 'public.added_later()', 'execute') as authenticated,
                    has_function_privilege('service_role', 'public.added_later()', 'execute') as service_role`
            await sql`drop function public.added_later()`
            assert.deepEqual({ ...privileges }, { anon: false, authenticated: false, service_role: true })
        })

        await t.test('the browser keys have no rights on any table or sequence', async () => {
            const relations = await sql`
                select c.relname::text as name from pg_class c
                cross join lateral aclexplode(c.relacl) acl
                where c.relnamespace = 'public'::regnamespace and acl.grantee in ('anon'::regrole, 'authenticated'::regrole, 0::oid)`
            assert.deepEqual(relations.map((row: { name: string }) => row.name), [])
            const [{ opened }] = await sql`
                select count(*) as opened from pg_attribute a join pg_class c on c.oid = a.attrelid
                where c.relnamespace = 'public'::regnamespace and a.attacl is not null`
            assert.equal(Number(opened), 0)
        })

        await t.test('a table added later is only the server\'s until it is granted to the browser keys', async () => {
            await sql`create table public.added_later (id bigserial primary key)`
            const tableRights = 'select, insert, update, delete, truncate, references, trigger'
            const sequenceRights = 'usage, select, update'
            const [privileges] = await sql`
                select has_table_privilege('anon', 'public.added_later', ${tableRights}) as anon_table,
                    has_table_privilege('authenticated', 'public.added_later', ${tableRights}) as authenticated_table,
                    has_table_privilege('service_role', 'public.added_later', ${tableRights}) as service_role_table,
                    has_sequence_privilege('anon', 'public.added_later_id_seq', ${sequenceRights}) as anon_sequence,
                    has_sequence_privilege('authenticated', 'public.added_later_id_seq', ${sequenceRights}) as authenticated_sequence,
                    has_sequence_privilege('service_role', 'public.added_later_id_seq', ${sequenceRights}) as service_role_sequence`
            await sql`drop table public.added_later`
            assert.deepEqual({ ...privileges }, {
                anon_table: false, authenticated_table: false, service_role_table: true,
                anon_sequence: false, authenticated_sequence: false, service_role_sequence: true,
            })
        })

        await t.test("only the game's current editor saves its world, and a dev editor skips the check", async () => {
            const gameId = await addGame(await addProfile())
            await sql`insert into starting_editor_servers (request_id, game_id) values ('editor-1', ${gameId})`
            const save = (requestId: string | null) => sql`select editor_save_world(${gameId}, ${requestId}, gen_random_uuid())`
            await save('editor-1')
            assert.equal(await codeOf(save('editor-2')), 'PT410')
            await save(null)
            await sql`select editor_remove_deployment(${gameId}, 'editor-1')`
            assert.equal(await codeOf(save('editor-1')), 'PT410')
        })

        await t.test("removing a deployment leaves the editor that replaced it", async () => {
            const gameId = await addGame(await addProfile())
            await sql`insert into running_editor_servers (request_id, game_id, editor_url, public_ip, playtest_external_port) values ('old-editor', ${gameId}, 'https://old.example', '127.0.0.1', 30001)`
            await sql`insert into starting_editor_servers (request_id, game_id) values ('replacement-editor', ${gameId})`
            await sql`select editor_remove_deployment(${gameId}, 'old-editor')`
            assert.equal((await sql`select 1 from running_editor_servers where request_id = 'old-editor'`).length, 0)
            assert.equal((await sql`select 1 from starting_editor_servers where request_id = 'replacement-editor'`).length, 1)
        })

        await t.test("loading reads the game's creator, its editor world and its latest publication", async () => {
            const creatorId = await addProfile()
            const gameId = await addGame(creatorId)
            const load = async () => ({ ...(await sql`select * from editor_load_world(${gameId}, null)`)[0] })
            assert.deepEqual(await load(), { creator_id: creatorId, editor_world_uuid: null, published_world_uuid: null })
            await sql`insert into game_worlds (game_id, world_uuid) values (${gameId}, gen_random_uuid()), (${gameId}, gen_random_uuid())`
            const [latest] = await sql`select world_uuid from game_worlds where game_id = ${gameId} order by version_num desc limit 1`
            assert.equal((await load()).published_world_uuid, latest.world_uuid)
            const editorWorldUuid = randomUUID()
            await sql`select editor_save_world(${gameId}, null, ${editorWorldUuid}::uuid)`
            assert.equal((await load()).editor_world_uuid, editorWorldUuid)
            assert.equal(await codeOf(sql`select * from editor_load_world(${gameId}, 'another-editor')`), 'PT410')
            assert.equal(await codeOf(sql`select * from editor_load_world(-1, null)`), 'PT404')
            await sql`insert into starting_editor_servers (request_id, game_id) values ('starting-loader', ${gameId})`
            assert.equal((await sql`select * from editor_load_world(${gameId}, 'starting-loader')`)[0].creator_id, creatorId)
            await sql`delete from starting_editor_servers where request_id = 'starting-loader'`
            await sql`insert into running_editor_servers (request_id, game_id, editor_url, public_ip, playtest_external_port) values ('running-loader', ${gameId}, 'https://editor.example', '127.0.0.1', 30002)`
            assert.equal((await sql`select * from editor_load_world(${gameId}, 'running-loader')`)[0].creator_id, creatorId)
        })

        await t.test("the browser gets in as the creator or with the game's invite, while its editor runs", async () => {
            const creatorId = await addProfile()
            const gameId = await addGame(creatorId)
            const access = (requestId: string | null, profileId: string | null, inviteToken: string | null) =>
                sql`select editor_browser_access(${gameId}, ${requestId}, ${profileId}::uuid, ${inviteToken}) as creator_id`
            assert.equal((await access(null, creatorId, null))[0].creator_id, creatorId)
            assert.equal(await codeOf(access(null, await addProfile(), null)), 'PT403')
            await sql`insert into editor_share_auth (game_id, invite_token) values (${gameId}, 'invite-1')`
            assert.equal((await access(null, null, 'invite-1'))[0].creator_id, creatorId)
            assert.equal(await codeOf(access(null, null, 'invite-2')), 'PT403')
            await sql`update editor_share_auth set invite_token = '' where game_id = ${gameId}`
            assert.equal(await codeOf(access(null, null, '')), 'PT403')
            assert.equal(await codeOf(access('editor-3', creatorId, null)), 'PT410')
            await sql`insert into starting_editor_servers (request_id, game_id) values ('editor-3', ${gameId})`
            assert.equal(await codeOf(access('editor-3', creatorId, null)), 'PT410')
            await sql`delete from starting_editor_servers where request_id = 'editor-3'`
            await sql`insert into running_editor_servers (request_id, game_id, editor_url, public_ip, playtest_external_port) values ('editor-3', ${gameId}, 'https://editor.example', '127.0.0.1', 30000)`
            assert.equal((await access('editor-3', creatorId, null))[0].creator_id, creatorId)
        })

        await t.test("a site admin gets into any game's editor, and the browser keys can't touch the admin list", async () => {
            const creatorId = await addProfile()
            const gameId = await addGame(creatorId)
            const adminId = await addProfile()
            const access = (profileId: string) => sql`select editor_browser_access(${gameId}, null, ${profileId}::uuid, null) as creator_id`
            assert.equal(await codeOf(access(adminId)), 'PT403')
            await sql`insert into site_admins (profile_id) values (${adminId}::uuid)`
            assert.equal((await access(adminId))[0].creator_id, creatorId)
            const [privileges] = await sql`
                select has_table_privilege('anon', 'public.site_admins', 'select, insert, update, delete, truncate') as anon,
                    has_table_privilege('authenticated', 'public.site_admins', 'select, insert, update, delete, truncate') as authenticated`
            assert.deepEqual({ ...privileges }, { anon: false, authenticated: false })
        })

        await t.test('a turn numbers its user message and reply after the history, and a resumed turn adds only the reply', async () => {
            const gameId = await addGame(await addProfile())
            const message = (id: string) => sql.json({ id, role: 'user', parts: [] })
            await sql`select start_chat_reply(${gameId}, ${message('user-1')}, ${message('reply-1')})`
            await sql`select start_chat_reply(${gameId}, null, ${message('reply-2')})`
            const rows = await sql`select seq, message->>'id' as id from chat_messages where game_id = ${gameId} order by seq`
            assert.deepEqual(rows.map((row: { seq: string, id: string }) => [Number(row.seq), row.id]), [[1, 'user-1'], [2, 'reply-1'], [3, 'reply-2']])
        })

        await t.test('turns started at the same moment get their own numbers', async () => {
            const gameId = await addGame(await addProfile())
            const message = (id: string) => sql.json({ id, role: 'user', parts: [] })
            await Promise.all(Array.from({ length: 10 }, (_, index) => sql`select start_chat_reply(${gameId}, ${message(`user-${index}`)}, ${message(`reply-${index}`)})`))
            const seqs = (await sql`select seq from chat_messages where game_id = ${gameId} order by seq`).map((row: { seq: string }) => Number(row.seq))
            assert.deepEqual(seqs, Array.from({ length: 20 }, (_, index) => index + 1))
        })

        await t.test('the history since the latest summary starts at that summary, or at the beginning without one', async () => {
            const gameId = await addGame(await addProfile())
            const summary = (text: string) => [{ type: 'data-compaction', data: { summary: text } }]
            const messages = [
                { id: 'a', role: 'user', parts: [] },
                { id: 'b', role: 'assistant', parts: summary('First summary') },
                { id: 'c', role: 'user', parts: [] },
                { id: 'd', role: 'assistant', parts: summary('Second summary') },
                { id: 'e', role: 'user', parts: [] },
            ]
            const idsSinceSummary = async () => (await sql`select message->>'id' as id from chat_messages_since_summary(${gameId}) as message`).map((row: { id: string }) => row.id)
            await sql`insert into chat_messages (game_id, seq, message) values (${gameId}, 1, ${sql.json(messages[0])})`
            assert.deepEqual(await idsSinceSummary(), ['a'])
            for (const [index, message] of messages.slice(1).entries())
                await sql`insert into chat_messages (game_id, seq, message) values (${gameId}, ${index + 2}, ${sql.json(message)})`
            assert.deepEqual(await idsSinceSummary(), ['d', 'e'])
        })

        await t.test("a catalog item needs every mesh to be its creator's, and its creator owns it for nothing", async () => {
            const creatorId = await addProfile()
            const addMesh = async (uploadedBy: string) => Number((await sql`insert into data_metadata (data_type, uploaded_by) values ('mesh_data', ${uploadedBy}::uuid) returning data_id`)[0].data_id)
            const ownMeshes = [await addMesh(creatorId), await addMesh(creatorId)]
            const strangerMesh = await addMesh(await addProfile())
            const publish = (catalogItemId: number, meshDataIds: number[]) =>
                sql`select * from publish_catalog_item(${catalogItemId}, ${creatorId}::uuid, 'Hat', '', true, ${meshDataIds}::bigint[])`
            assert.equal(await codeOf(publish(9001, [...ownMeshes, strangerMesh])), 'PT403')
            const [item] = await publish(9002, ownMeshes)
            assert.deepEqual({ catalogItemId: Number(item.catalog_item_id), description: item.description, isListed: item.is_listed }, { catalogItemId: 9002, description: null, isListed: true })
            const owned = await sql`select 1 from user_owned_catalog_items where profile_id = ${creatorId}::uuid and catalog_item_id = 9002`
            assert.equal(owned.length, 1)
        })

        await t.test('importing an asset version counts it', async () => {
            const [asset] = await sql`insert into asset_metadata (uploaded_by, name, asset_type, asset_uuid) values (${await addProfile()}::uuid, 'Rock', 'Mesh', gen_random_uuid()) returning asset_id, version_num`
            await sql`select record_asset_import(${asset.asset_id}, ${asset.version_num})`
            await sql`select record_asset_import(${asset.asset_id}, ${asset.version_num})`
            const [counted] = await sql`select import_count from asset_metadata where asset_id = ${asset.asset_id}`
            assert.equal(Number(counted.import_count), 2)
        })

        const usageGameId = await addGame(await addProfile())
        const charge = (profileId: string, awsRequestId: string, credits: number, messageId: string | null = 'message-1', outputTokens: number | null = 400) =>
            sql`select charge_chat_usage(${profileId}::uuid, ${usageGameId}, ${messageId}, 'openai.gpt-6-astra', 1000, 300, 2000, ${outputTokens}, ${awsRequestId}, ${credits})`
        const readCreditsUsedToday = async (profileId: string) => Number((await sql`select credits_used_today(${profileId}::uuid) as credits`)[0].credits)
        const readUsage = (profileId: string) => sql`select message_id, game_id, model_id, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, aws_request_id, credits_charged from chat_usage where profile_id = ${profileId}::uuid order by id`

        await t.test('a model call charged twice is charged once, with its tokens and game', async () => {
            const profileId = await addProfile()
            await charge(profileId, 'response-1', 10)
            await charge(profileId, 'response-1', 10)
            assert.equal(await readCreditsUsedToday(profileId), 10)
            assert.deepEqual((await readUsage(profileId)).map((row: object) => ({ ...row })), [{
                message_id: 'message-1', game_id: String(usageGameId), model_id: 'openai.gpt-6-astra', input_tokens: 1000, cache_read_tokens: 2000,
                cache_write_tokens: 300, output_tokens: 400, aws_request_id: 'response-1', credits_charged: 10,
            }])
        })

        await t.test('the credits a profile used today are its own calls since midnight UTC, each in full', async () => {
            const profileId = await addProfile()
            await charge(profileId, 'response-today-1', 9_000)
            await charge(profileId, 'response-today-2', 5_000)
            await charge(profileId, 'response-yesterday', 7)
            await sql`update chat_usage set created_at = date_trunc('day', now(), 'UTC') - interval '1 second' where aws_request_id = 'response-yesterday'`
            await charge(await addProfile(), 'response-someone-else', 3)
            assert.equal(await readCreditsUsedToday(profileId), 14_000)
            assert.equal(await readCreditsUsedToday(randomUUID()), 0)
        })

        await t.test('calls that finish together are all charged', async () => {
            const profileId = await addProfile()
            await Promise.all(Array.from({ length: 10 }, (_, index) => charge(profileId, `response-together-${index}`, 3)))
            assert.equal(await readCreditsUsedToday(profileId), 30)
            assert.equal((await readUsage(profileId)).length, 10)
        })

        await t.test('a call whose output was never reported stores no output tokens', async () => {
            const profileId = await addProfile()
            await charge(profileId, 'response-no-output', 1, null, null)
            const [usage] = await readUsage(profileId)
            assert.equal(usage.output_tokens, null)
            assert.equal(usage.message_id, null)
        })
    } finally {
        await sql.end()
        await database.stop()
    }
})
