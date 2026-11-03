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

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The route the desktop app polls for its server's address after
// Play runs on a throwaway copy of production's schema, while the ready webhook moves that server from starting to running.
test('a player gets the address of a server that starts running while they ask for it', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseOnPoolerPortForTests()
    const directory = await mkdtemp(path.join(tmpdir(), 'connection-info-'))
    process.env.DRIZZLE_DATABASE_URL = database.url
    // Bundled like the website builds it, so '@/' imports resolve and the route runs through the website's own database module.
    // CommonJS, because the copy of Next's code the route imports reads __dirname.
    const { build } = createRequire(repoPath('packages/website/package.json'))('esbuild')
    const website = repoPath('packages/website')
    await build({
        stdin: { contents: `export { GET } from '@/app/api/play-sessions/[requestId]/connection-info/route'\nexport { hashDesktopAuthToken } from '@/lib/desktopAuth'\nexport { sql as websiteSql } from '@/lib/db'\n`, resolveDir: website, loader: 'ts' },
        outfile: path.join(directory, 'connectionInfo.cjs'), bundle: true, format: 'cjs', platform: 'node',
        tsconfig: path.join(website, 'tsconfig.json'),
    })
    const { GET, hashDesktopAuthToken, websiteSql } = await import(pathToFileURL(path.join(directory, 'connectionInfo.cjs')).href)
    // The website's database module has one connection, which the route uses, so the promotion and the lock checks use their own.
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
        const [world] = await sql`insert into game_worlds (game_id, world_uuid) values (${game.game_id}, gen_random_uuid()) returning version_num`
        let playerCount = 0
        // A player who pressed Play and got a seat on a new server that Edgegap is still starting.
        const addPlayerOnStartingServer = async () => {
            playerCount += 1
            const [profileId, requestId, playAuth] = [randomUUID(), `server-${playerCount}`, `play-auth-${playerCount}`]
            await sql`insert into user_profiles (id, username) values (${profileId}::uuid, ${`player${playerCount}`})`
            await sql`insert into user_desktop_play_auth (profile_id, play_auth_hash) values (${profileId}::uuid, ${hashDesktopAuthToken(playAuth)})`
            await sql`insert into starting_servers (request_id, game_id, gameworld_version_num, max_players, starting_at) values (${requestId}, ${game.game_id}, ${world.version_num}, 8, now())`
            await sql`insert into starting_player_sessions (profile_id, request_id) values (${profileId}::uuid, ${requestId})`
            return { requestId, playAuth }
        }
        const askForAddress = async ({ requestId, playAuth }: { requestId: string, playAuth: string }) => {
            const response = await GET(new Request(`http://localhost/api/play-sessions/${requestId}/connection-info`, { headers: { authorization: `Bearer ${playAuth}` } }), { params: Promise.resolve({ requestId }) })
            return { status: response.status, body: await response.json() }
        }
        // Moves the server and its seats from starting to running inside a transaction, like the ready webhook does.
        const promote = async (promotion: typeof sql, requestId: string) => {
            await promotion`insert into running_servers (request_id, game_id, gameworld_version_num, max_players, public_ip, external_port) select request_id, game_id, gameworld_version_num, max_players, '203.0.113.7', 7777 from starting_servers where request_id = ${requestId}`
            await promotion`insert into running_player_sessions (profile_id, request_id) select profile_id, request_id from starting_player_sessions where request_id = ${requestId}`
            await promotion`delete from starting_servers where request_id = ${requestId}`
        }
        // The promotion locks one table first and makes its changes once the route is waiting to read that table, so the route
        // reads that table after the promotion and everything before it from before.
        const promoteWhileRouteWaitsOn = async (table: string, player: { requestId: string, playAuth: string }) => {
            let answer: Promise<{ status: number, body: unknown }> | undefined
            let isAnswered = false
            await sql.begin(async (promotion) => {
                await promotion`lock table ${promotion(table)} in access exclusive mode`
                answer = askForAddress(player).finally(() => { isAnswered = true })
                while (!isAnswered && (await sql`select 1 from pg_stat_activity where wait_event_type = 'Lock'`).length === 0) {}
                await promote(promotion, player.requestId)
            })
            return answer
        }

        await t.test('the server starts running while the route waits to read the starting seat', async () => {
            const player = await addPlayerOnStartingServer()
            assert.deepEqual(await promoteWhileRouteWaitsOn('starting_player_sessions', player), { status: 200, body: { public_ip: '203.0.113.7', port: 7777 } })
        })

        await t.test('the server starts running while the route waits to read the running servers', async () => {
            const player = await addPlayerOnStartingServer()
            assert.deepEqual(await promoteWhileRouteWaitsOn('running_servers', player), { status: 200, body: { public_ip: '203.0.113.7', port: 7777 } })
        })

        await t.test('a server that stopped while starting is gone', async () => {
            const player = await addPlayerOnStartingServer()
            await sql`delete from starting_servers where request_id = ${player.requestId}`
            assert.deepEqual(await askForAddress(player), { status: 410, body: { error: 'Server is no longer available' } })
        })

        await t.test('a seat on one server does not give out the address of another', async () => {
            const player = await addPlayerOnStartingServer()
            const otherPlayer = await addPlayerOnStartingServer()
            await sql.begin(promotion => promote(promotion, otherPlayer.requestId))
            assert.deepEqual(await askForAddress({ requestId: otherPlayer.requestId, playAuth: player.playAuth }), { status: 410, body: { error: 'Server is no longer available' } })
        })
    } finally {
        await sql.end()
        await websiteSql.end()
        await database.stop()
        await rm(directory, { recursive: true, force: true })
    }
})
