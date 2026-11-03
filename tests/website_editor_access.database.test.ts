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

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The website's own editor access query runs on a throwaway copy of
// production's schema.
test('the editor opens for its creator, for the invite of that one game, or for a site admin, and nobody else', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseOnPoolerPortForTests()
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-access-'))
    process.env.DRIZZLE_DATABASE_URL = database.url
    // Bundled like the website builds it, so '@/' imports resolve and the query runs through the website's own database module.
    const { build } = createRequire(repoPath('packages/website/package.json'))('esbuild')
    const website = repoPath('packages/website')
    await build({
        stdin: { contents: `export { getEditorAccess } from '@/lib/editorAccess'\nexport { sql } from '@/lib/db'\n`, resolveDir: website, loader: 'ts' },
        outfile: path.join(directory, 'editorAccess.mjs'), bundle: true, format: 'esm', platform: 'node',
        tsconfig: path.join(website, 'tsconfig.json'),
    })
    const { getEditorAccess, sql } = await import(pathToFileURL(path.join(directory, 'editorAccess.mjs')).href)
    try {
        const [creatorId, strangerId] = [randomUUID(), randomUUID()]
        await sql`insert into user_profiles (id, username) values (${creatorId}::uuid, 'creator1'), (${strangerId}::uuid, 'stranger1')`
        const addGame = async (name: string, isPublished: boolean, inviteToken: string | null) => {
            const [game] = await sql`insert into game_metadata (name, creator_id, max_players, is_published) values (${name}, ${creatorId}::uuid, 8, ${isPublished}) returning game_id`
            await sql`insert into editor_share_auth (game_id, invite_token) values (${game.game_id}, ${inviteToken})`
            return Number(game.game_id)
        }
        const sharedGameId = await addGame('Shared', true, 'invite-for-shared')
        await sql`insert into game_worlds (game_id, world_uuid) values (${sharedGameId}, gen_random_uuid())`
        const hiddenGameId = await addGame('Hidden', false, null)
        const emptyTokenGameId = await addGame('Empty', false, '')

        await t.test('the creator opens their game without an invite', async () => {
            assert.deepEqual({ ...await getEditorAccess(sharedGameId, creatorId, null) }, { name: 'Shared', description: null, suggestedMinPlayers: 1, maxPlayers: 8, creatorId, isCreator: true, isPublished: true, thumbnailUuid: null, isPublic: true })
            assert.deepEqual({ ...await getEditorAccess(hiddenGameId, creatorId, null) }, { name: 'Hidden', description: null, suggestedMinPlayers: 1, maxPlayers: 8, creatorId, isCreator: true, isPublished: false, thumbnailUuid: null, isPublic: false })
        })

        await t.test('an invite opens only the game it was made for', async () => {
            assert.deepEqual({ ...await getEditorAccess(sharedGameId, strangerId, 'invite-for-shared') }, { name: 'Shared', description: null, suggestedMinPlayers: 1, maxPlayers: 8, creatorId, isCreator: false, isPublished: true, thumbnailUuid: null, isPublic: true })
            assert.deepEqual({ ...await getEditorAccess(sharedGameId, null, 'invite-for-shared') }, { name: 'Shared', description: null, suggestedMinPlayers: 1, maxPlayers: 8, creatorId, isCreator: false, isPublished: true, thumbnailUuid: null, isPublic: true })
            assert.equal(await getEditorAccess(hiddenGameId, strangerId, 'invite-for-shared'), undefined)
        })

        await t.test('no invite, a wrong one, a hidden one, and an empty one all stay closed', async () => {
            assert.equal(await getEditorAccess(sharedGameId, strangerId, null), undefined)
            assert.equal(await getEditorAccess(sharedGameId, strangerId, 'invite-for-shared-but-longer'), undefined)
            assert.equal(await getEditorAccess(hiddenGameId, null, null), undefined)
            assert.equal(await getEditorAccess(emptyTokenGameId, strangerId, ''), undefined)
        })

        await t.test('a site admin opens any game without an invite, but not as its creator', async () => {
            const adminId = randomUUID()
            await sql`insert into user_profiles (id, username) values (${adminId}::uuid, 'admin1')`
            assert.equal(await getEditorAccess(hiddenGameId, adminId, null), undefined)
            await sql`insert into site_admins (profile_id) values (${adminId}::uuid)`
            assert.deepEqual({ ...await getEditorAccess(hiddenGameId, adminId, null) }, { name: 'Hidden', description: null, suggestedMinPlayers: 1, maxPlayers: 8, creatorId, isCreator: false, isPublished: false, thumbnailUuid: null, isPublic: false })
            assert.equal(await getEditorAccess(hiddenGameId, strangerId, null), undefined)
        })
    } finally {
        await sql.end()
        await database.stop()
        await rm(directory, { recursive: true, force: true })
    }
})
