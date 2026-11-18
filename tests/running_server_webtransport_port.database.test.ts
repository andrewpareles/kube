import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. A browser joins a running server on the port Edgegap
// published for it, so the running server's row has to carry it, and a deployment that published none has to keep
// working for desktop players.
test('a running server keeps the browser port it was published and may have none', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
        const [world] = await sql`insert into game_worlds (game_id, world_uuid) values (${game.game_id}, gen_random_uuid()) returning version_num`

        await sql`insert into running_servers (request_id, game_id, gameworld_version_num, max_players, public_ip, external_port, webtransport_port) values ('server-1', ${game.game_id}, ${world.version_num}, 8, '127.0.0.1', 7777, 7778)`
        await sql`insert into running_servers (request_id, game_id, gameworld_version_num, max_players, public_ip, external_port) values ('server-2', ${game.game_id}, ${world.version_num}, 8, '127.0.0.1', 7779)`

        const [browserServer] = await sql`select external_port, webtransport_port from running_servers where request_id = 'server-1'`
        assert.equal(browserServer.external_port, 7777)
        assert.equal(browserServer.webtransport_port, 7778)

        const [desktopOnlyServer] = await sql`select external_port, webtransport_port from running_servers where request_id = 'server-2'`
        assert.equal(desktopOnlyServer.external_port, 7779)
        assert.equal(desktopOnlyServer.webtransport_port, null)
    } finally {
        await sql.end()
        await database.stop()
        await t.diagnostic(`database ${database.url} stopped`)
    }
})
