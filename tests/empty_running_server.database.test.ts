import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The call an empty game server makes before it stops itself
// runs on a throwaway copy of production's schema.
test('an empty game server leaves the running servers only when nobody is in it or on the way', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
        const [world] = await sql`insert into game_worlds (game_id, world_uuid) values (${game.game_id}, gen_random_uuid()) returning version_num`
        let serverCount = 0
        const addServer = async () => {
            serverCount += 1
            const requestId = `server-${serverCount}`
            await sql`insert into running_servers (request_id, game_id, gameworld_version_num, max_players, public_ip, external_port) values (${requestId}, ${game.game_id}, ${world.version_num}, 8, '127.0.0.1', 7777)`
            return requestId
        }
        let profileCount = 0
        // A player with a seat on the server, reserved this many seconds ago, and let in by the game server or not yet.
        const addPlayer = async (requestId: string, { reservedSecondsAgo, isConnected }: { reservedSecondsAgo: number, isConnected: boolean }) => {
            profileCount += 1
            const profileId = randomUUID()
            await sql`insert into user_profiles (id, username) values (${profileId}::uuid, ${`player${profileCount}`})`
            await sql`insert into running_player_sessions (profile_id, request_id, reserved_at, connected_at) values (${profileId}::uuid, ${requestId}, now() - make_interval(secs => ${reservedSecondsAgo}), ${isConnected ? sql`now()` : null})`
        }
        const deleteIfEmpty = async (requestId: string) => (await sql`select delete_running_server_if_empty(${requestId}) as is_deleted`)[0].is_deleted
        const isRunning = async (requestId: string) => (await sql`select 1 from running_servers where request_id = ${requestId}`).length === 1

        await t.test('a server with nobody in it is removed', async () => {
            const requestId = await addServer()
            assert.equal(await deleteIfEmpty(requestId), true)
            assert.equal(await isRunning(requestId), false)
        })

        await t.test('a player in the server keeps it', async () => {
            const requestId = await addServer()
            await addPlayer(requestId, { reservedSecondsAgo: 600, isConnected: true })
            assert.equal(await deleteIfEmpty(requestId), false)
            assert.equal(await isRunning(requestId), true)
        })

        await t.test('a player who was just given a seat and has not arrived yet keeps it', async () => {
            const requestId = await addServer()
            await addPlayer(requestId, { reservedSecondsAgo: 5, isConnected: false })
            assert.equal(await deleteIfEmpty(requestId), false)
            assert.equal(await isRunning(requestId), true)
        })

        await t.test('a seat nobody arrived for in time does not keep it, and goes with it', async () => {
            const requestId = await addServer()
            await addPlayer(requestId, { reservedSecondsAgo: 60, isConnected: false })
            assert.equal(await deleteIfEmpty(requestId), true)
            assert.equal(await isRunning(requestId), false)
            assert.equal((await sql`select 1 from running_player_sessions where request_id = ${requestId}`).length, 0)
        })

        await t.test('players in another server do not keep it', async () => {
            const requestId = await addServer()
            const otherRequestId = await addServer()
            await addPlayer(otherRequestId, { reservedSecondsAgo: 600, isConnected: true })
            assert.equal(await deleteIfEmpty(requestId), true)
            assert.equal(await isRunning(otherRequestId), true)
        })

        await t.test('a launch that is giving out a seat finishes first, and its player keeps the server', async () => {
            const requestId = await addServer()
            const profileId = randomUUID()
            await sql`insert into user_profiles (id, username) values (${profileId}::uuid, 'launching')`
            let isDeleted: Promise<boolean> | undefined
            let isFinished = false
            await sql.begin(async (launch) => {
                await launch`select 1 from game_metadata where game_id = ${game.game_id} for update`
                isDeleted = deleteIfEmpty(requestId).finally(() => { isFinished = true })
                while (!isFinished && (await sql`select 1 from pg_stat_activity where wait_event_type = 'Lock' and query like '%delete_running_server_if_empty%'`).length === 0) {}
                await launch`insert into running_player_sessions (profile_id, request_id) values (${profileId}::uuid, ${requestId})`
            })
            assert.equal(await isDeleted, false)
            assert.equal(await isRunning(requestId), true)
        })

        await t.test('a server the website still lists as starting keeps running', async () => {
            await sql`insert into starting_servers (request_id, game_id, gameworld_version_num, max_players, starting_at) values ('starting-server', ${game.game_id}, ${world.version_num}, 8, now())`
            assert.equal(await deleteIfEmpty('starting-server'), false)
        })

        await t.test('a server that is already gone can stop', async () => {
            assert.equal(await deleteIfEmpty('server-that-was-never-added'), true)
        })
    } finally {
        await sql.end()
        await database.stop()
    }
})
