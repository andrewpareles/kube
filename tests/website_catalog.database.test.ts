import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. Buying from the catalog runs on a throwaway copy of production's
// schema with the pending migrations applied.
test('buying a catalog item is free and happens once, and owners wear one item they own', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { max: 25, onnotice: () => {} })
    let profileCount = 0
    const addProfile = async () => {
        profileCount += 1
        const [profile] = await sql`insert into user_profiles (id, username) values (${randomUUID()}, ${`player${profileCount}`}) returning id`
        return String(profile.id)
    }
    const addItem = async (creatorId: string, isListed = true) => {
        const [item] = await sql`
            insert into catalog_metadata (creator_id, name, is_listed)
            values (${creatorId}::uuid, 'Hat', ${isListed})
            returning catalog_item_id
        `
        return Number(item.catalog_item_id)
    }
    const buy = (buyerId: string, catalogItemId: number) => sql`select buy_catalog_item(${buyerId}::uuid, ${catalogItemId}::bigint)`
    const ownsItem = async (profileId: string, catalogItemId: number) => (await sql`select 1 from user_owned_catalog_items where profile_id = ${profileId}::uuid and catalog_item_id = ${catalogItemId}`).length === 1
    try {
        await t.test('a purchase records that the buyer owns the item', async () => {
            const buyerId = await addProfile()
            const itemId = await addItem(await addProfile())
            await buy(buyerId, itemId)
            assert.equal(await ownsItem(buyerId, itemId), true)
        })

        await t.test('an item is bought once, even when two purchases of it arrive together', async () => {
            const buyerId = await addProfile()
            const itemId = await addItem(await addProfile())
            const results = await Promise.allSettled([buy(buyerId, itemId), buy(buyerId, itemId)])
            assert.deepEqual(results.map(result => result.status).sort(), ['fulfilled', 'rejected'])
            await assert.rejects(buy(buyerId, itemId), /You already own this item/)
        })

        await t.test('an unlisted item cannot be bought', async () => {
            const buyerId = await addProfile()
            const itemId = await addItem(await addProfile(), false)
            await assert.rejects(buy(buyerId, itemId), /This item is not for sale/)
            assert.equal(await ownsItem(buyerId, itemId), false)
        })

        await t.test('people buying from each other at the same time all get what they bought', async () => {
            const profileIds = await Promise.all(Array.from({ length: 6 }, () => addProfile()))
            const itemIds = await Promise.all(profileIds.map(profileId => addItem(profileId)))
            await Promise.all(profileIds.flatMap((buyerId, buyerIndex) => itemIds.filter((_, itemIndex) => itemIndex !== buyerIndex).map(itemId => buy(buyerId, itemId))))
            const [{ owned }] = await sql`select count(*) as owned from user_owned_catalog_items where profile_id = any(${profileIds}::uuid[])`
            assert.equal(Number(owned), 30)
        })

        await t.test('only the server can buy, and deleting an item takes it from its owners', async () => {
            const buyerId = await addProfile()
            const itemId = await addItem(await addProfile())
            await assert.rejects(sql.begin(async (transaction: typeof sql) => {
                await transaction`set local role authenticated`
                await transaction`select buy_catalog_item(${buyerId}::uuid, ${itemId}::bigint)`
            }), /permission denied/)
            await buy(buyerId, itemId)
            await sql`update user_profiles set equipped_catalog_item_id = ${itemId} where id = ${buyerId}::uuid`
            await sql`delete from catalog_metadata where catalog_item_id = ${itemId}`
            assert.equal(await ownsItem(buyerId, itemId), false)
            const [after] = await sql`select equipped_catalog_item_id from user_profiles where id = ${buyerId}::uuid`
            assert.equal(after.equipped_catalog_item_id, null)
        })

        await t.test('a profile wears only an item it owns, and can still be deleted while wearing it', async () => {
            const creatorId = await addProfile()
            const buyerId = await addProfile()
            const [firstItemId, secondItemId] = [await addItem(creatorId), await addItem(creatorId)]
            const equip = (profileId: string, catalogItemId: number) => sql`update user_profiles set equipped_catalog_item_id = ${catalogItemId} where id = ${profileId}::uuid`
            await assert.rejects(equip(buyerId, firstItemId), /user_profiles_equipped_item_is_owned/)
            await buy(buyerId, firstItemId)
            await buy(buyerId, secondItemId)
            await equip(buyerId, firstItemId)
            await equip(buyerId, secondItemId)
            const [profile] = await sql`select equipped_catalog_item_id from user_profiles where id = ${buyerId}::uuid`
            assert.equal(Number(profile.equipped_catalog_item_id), secondItemId)
            await sql`delete from user_profiles where id = ${buyerId}::uuid`
            assert.equal(await ownsItem(buyerId, secondItemId), false)
        })

        await t.test('browsers read neither the catalog nor their own purchases directly', async () => {
            const buyerId = await addProfile()
            const itemId = await addItem(await addProfile())
            await buy(buyerId, itemId)
            await assert.rejects(sql.begin(async (transaction: typeof sql) => {
                await transaction`set local role anon`
                await transaction`select catalog_item_id from catalog_metadata where catalog_item_id = ${itemId}`
            }), /permission denied/)
            await assert.rejects(sql.begin(async (transaction: typeof sql) => {
                await transaction`select set_config('request.jwt.claim.sub', ${buyerId}, true)`
                await transaction`set local role authenticated`
                await transaction`select catalog_item_id from user_owned_catalog_items where catalog_item_id = ${itemId}`
            }), /permission denied/)
        })
    } finally {
        await sql.end()
        await database.stop()
    }
})
