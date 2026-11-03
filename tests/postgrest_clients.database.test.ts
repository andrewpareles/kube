import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { test } from 'node:test'
import { promisify } from 'node:util'
import { findFreePort, startDatabaseForTests, startPostgrestForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

const run = promisify(execFile)

// Opt in with DATABASE_TESTS=1, Postgres 17 first on PATH and PostgREST. The Rust tests of the database calls game servers and
// editors make each get PostgREST over their own throwaway copy of production's schema, which these start.
test('the game server and editor database calls work through PostgREST', { skip: process.env.DATABASE_TESTS !== '1', timeout: 30 * 60_000 }, async (t) => {
    for (const [crate, testTarget] of [['server', 'database_tests'], ['editor_server', 'database_tests']]) {
        await t.test(`cargo test -p ${crate} --test ${testTarget}`, async () => {
            const database = await startDatabaseForTests(await findFreePort())
            const postgrest = await startPostgrestForTests(database.url)
            try {
                const { stdout } = await run('cargo', ['test', '-p', crate, '--test', testTarget, '--', '--ignored'], {
                    cwd: repoPath('.'),
                    env: {
                        ...process.env,
                        CARGO_TERM_COLOR: 'never',
                        SUPABASE_URL: postgrest.url,
                        SUPABASE_SECRET_KEY: postgrest.serviceKey,
                        GAME_ID: '42',
                        ARBITRIUM_REQUEST_ID: 'this-deployment',
                        EDITOR_PLAYTEST_PORT: '30500',
                    },
                    maxBuffer: 64 * 1024 * 1024,
                }).catch((error: { stdout: string, stderr: string }) => {
                    throw new Error(`${error.stdout}\n${error.stderr}`)
                })
                assert.match(stdout, /test result: ok\. 1 passed/)
            } finally {
                await postgrest.stop()
                await database.stop()
            }
        })
    }
})
