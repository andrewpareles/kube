import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The cleanup a game server calls when a player leaves runs on a
// throwaway copy of production's schema.
test('a player leaving a server ends their session unless a newer launch has taken it over', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const creatorId = randomUUID()
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1')`
        const [game] = await sql`insert into game_metadata (name, creator_id, max_players) values ('Game', ${creatorId}::uuid, 8) returning game_id`
        const [world] = await sql`insert into game_worlds (game_id, world_uuid) values (${game.game_id}, gen_random_uuid()) returning version_num`
        for (const requestId of ['server-1', 'server-2']) {
            await sql`insert into running_servers (request_id, game_id, gameworld_version_num, max_players, public_ip, external_port) values (${requestId}, ${game.game_id}, ${world.version_num}, 8, '127.0.0.1', 7777)`
        }
        let profileCount = 0
        // A player in a session on server-1, launched with this play auth.
        const addPlayerInSession = async (playAuthHash: string | null) => {
            profileCount += 1
            const profileId = randomUUID()
            await sql`insert into user_profiles (id, username) values (${profileId}::uuid, ${`player${profileCount}`})`
            await sql`insert into user_desktop_play_auth (profile_id, play_auth_hash) values (${profileId}::uuid, ${playAuthHash})`
            await sql`insert into running_player_sessions (profile_id, request_id) values (${profileId}::uuid, 'server-1')`
            return profileId
        }
        const leave = (profileId: string, requestId: string, playAuthHash: string) => sql`select delete_running_player_session_unless_play_auth_changed(${profileId}::uuid, ${requestId}, ${playAuthHash})`
        const hasSession = async (profileId: string) => (await sql`select 1 from running_player_sessions where profile_id = ${profileId}::uuid`).length === 1

        await t.test('leaving with the play auth the session was launched with ends it', async () => {
            const profileId = await addPlayerInSession('launch-hash')
            await leave(profileId, 'server-1', 'launch-hash')
            assert.equal(await hasSession(profileId), false)
        })

        await t.test('a leave from a connection whose play auth a newer launch replaced keeps the new session', async () => {
            const profileId = await addPlayerInSession('newer-launch-hash')
            await leave(profileId, 'server-1', 'launch-hash')
            assert.equal(await hasSession(profileId), true)
        })

        await t.test('after logging out, any leave ends the session', async () => {
            const profileId = await addPlayerInSession(null)
            await leave(profileId, 'server-1', 'launch-hash')
            assert.equal(await hasSession(profileId), false)
        })

        await t.test('leaving another server leaves this session alone', async () => {
            const profileId = await addPlayerInSession('launch-hash')
            await leave(profileId, 'server-2', 'launch-hash')
            assert.equal(await hasSession(profileId), true)
        })
    } finally {
        await sql.end()
        await database.stop()
    }
})
