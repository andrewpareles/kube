import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. Runs on a throwaway copy of production's schema.
test('K coins are gone', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async () => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { onnotice: () => {} })
    try {
        const [{ count }] = await sql`
            select count(*) as count from information_schema.columns where table_schema = 'public' and column_name like '%k_coin%'
        `
        assert.equal(Number(count), 0)
        const [{ ledger, changeKCoins }] = await sql`select to_regclass('public.user_k_coin_ledger') as ledger, to_regproc('public.change_k_coins') as "changeKCoins"`
        assert.deepEqual([ledger, changeKCoins], [null, null])
    } finally {
        await sql.end()
        await database.stop()
    }
})
