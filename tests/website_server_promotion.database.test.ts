import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { pathToFileURL } from 'node:url'
import { startDatabaseOnPoolerPortForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The website moving a server from starting to running runs on a
// throwaway copy of production's schema, next to a launch of the same game that locks its player and then the game, like
// getJoinGameInfoAnyGame does.
test('a promotion and a launch of the same game wait for each other without deadlocking', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseOnPoolerPortForTests()
    const directory = await mkdtemp(path.join(tmpdir(), 'server-promotion-'))
    process.env.DRIZZLE_DATABASE_URL = database.url
    // Bundled like the website builds it, so '@/' imports resolve and the promotion runs through the website's own database module.
    const { build } = createRequire(repoPath('packages/website/package.json'))('esbuild')
    const website = repoPath('packages/website')
    await build({
        stdin: { contents: `export { promoteStartingServerToRunning } from '@/lib/serverTracking'\nexport { sql as websiteSql } from '@/lib/db'\n`, resolveDir: website, loader: 'ts' },
        outfile: path.join(directory, 'serverTracking.mjs'), bundle: true, format: 'esm', platform: 'node',
        tsconfig: path.join(website, 'tsconfig.json'),
    })
    const { promoteStartingServerToRunning, websiteSql } = await import(pathToFileURL(path.join(directory, 'serverTracking.mjs')).href)
    // The website's database module has one connection, which the promotion uses, so the launch and the lock checks use their own.
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
        const [world] = await sql`insert into game_worlds (game_id, world_uuid) values (${game.game_id}, gen_random_uuid()) returning version_num`
        let rowCount = 0
        const addStartingServer = async () => {
            rowCount += 1
            const requestId = `server-${rowCount}`
            await sql`insert into starting_servers (request_id, game_id, gameworld_version_num, max_players, starting_at) values (${requestId}, ${game.game_id}, ${world.version_num}, 8, now())`
            return requestId
        }
        const addPlayer = async () => {
            rowCount += 1
            const profileId = randomUUID()
            await sql`insert into user_profiles (id, username) values (${profileId}::uuid, ${`player${rowCount}`})`
            return profileId
        }
        // Starts the promotion and returns once it is waiting on a lock or done. The promotion comes back inside an object, so
        // awaiting this does not wait for the promotion to finish.
        const startPromotion = async (requestId: string) => {
            let isPromoted = false
            const promotion = promoteStartingServerToRunning({ requestId, publicIp: '203.0.113.7', externalPort: 7777 }).finally(() => { isPromoted = true })
            while (!isPromoted && (await sql`select 1 from pg_stat_activity where wait_event_type = 'Lock'`).length === 0) {}
            return { promotion }
        }
        const lockPlayerLikeALaunch = (launch: typeof sql, profileId: string) => launch`select 1 from user_profiles where id = ${profileId}::uuid for update`
        const lockGameLikeALaunch = (launch: typeof sql) => launch`select 1 from game_metadata where game_id = ${game.game_id} for update`
        const getRunningSeats = async (requestId: string) => (await sql`select profile_id from running_player_sessions where request_id = ${requestId}`).map((seat: { profile_id: string }) => seat.profile_id).sort()

        await t.test('a player who presses Play again while their server is promoted does not deadlock with it', async () => {
            const requestId = await addStartingServer()
            const profileId = await addPlayer()
            await sql`insert into starting_player_sessions (profile_id, request_id) values (${profileId}::uuid, ${requestId})`
            const { promotion } = await sql.begin(async (launch) => {
                await lockPlayerLikeALaunch(launch, profileId)
                const started = await startPromotion(requestId)
                await lockGameLikeALaunch(launch)
                return started
            })
            await promotion
            assert.deepEqual(await getRunningSeats(requestId), [profileId])
        })

        await t.test('a launch that reads the running servers and then the starting servers finds the server in one of them', async () => {
            const requestId = await addStartingServer()
            const { promotion, serverCounts } = await sql.begin(async (launch) => {
                await lockGameLikeALaunch(launch)
                const running = await launch`select 1 from running_servers where request_id = ${requestId}`
                const { promotion } = await startPromotion(requestId)
                const starting = await launch`select 1 from starting_servers where request_id = ${requestId}`
                return { promotion, serverCounts: { running: running.length, starting: starting.length } }
            })
            assert.deepEqual(serverCounts, { running: 0, starting: 1 })
            await promotion
            assert.equal((await sql`select 1 from running_servers where request_id = ${requestId}`).length, 1)
        })

        await t.test('a seat a launch gives out on the starting server moves to running with the others', async () => {
            const requestId = await addStartingServer()
            const [seatedPlayerId, launchingPlayerId] = [await addPlayer(), await addPlayer()]
            await sql`insert into starting_player_sessions (profile_id, request_id) values (${seatedPlayerId}::uuid, ${requestId})`
            const { promotion } = await sql.begin(async (launch) => {
                await lockPlayerLikeALaunch(launch, launchingPlayerId)
                await lockGameLikeALaunch(launch)
                await launch`insert into starting_player_sessions (profile_id, request_id) values (${launchingPlayerId}::uuid, ${requestId})`
                return startPromotion(requestId)
            })
            await promotion
            assert.deepEqual(await getRunningSeats(requestId), [seatedPlayerId, launchingPlayerId].sort())
            assert.equal((await sql`select 1 from starting_player_sessions where request_id = ${requestId}`).length, 0)
        })
    } finally {
        await sql.end()
        await websiteSql.end()
        await database.stop()
        await rm(directory, { recursive: true, force: true })
    }
})
