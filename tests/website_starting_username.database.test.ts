import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { createRequire } from 'node:module'
import { test } from 'node:test'
import { findFreePort, startDatabaseForTests } from './databaseForTests.ts'
import { repoPath } from './testPaths.ts'

// Opt in with DATABASE_TESTS=1 and Postgres 17 first on PATH. The dump has no auth schema, so auth.users here only has the
// columns handle_new_user reads, and the trigger is the one production puts on it.
test('every new account starts as Guest-<user_id>, and only that account can hold that name', { skip: process.env.DATABASE_TESTS !== '1', timeout: 120000 }, async (t) => {
    const database = await startDatabaseForTests(await findFreePort())
    const sql = createRequire(repoPath('packages/website/package.json'))('postgres')(database.url, { max: 1, onnotice: () => {} })
    const signUp = async (metadata: { username: string } | { full_name: string }) => {
        const id = randomUUID()
        await sql`insert into auth.users (id, email, raw_user_meta_data) values (${id}, ${`${id}@example.com`}, ${sql.json(metadata)})`
        const [profile] = await sql`select username, email, user_id from user_profiles where id = ${id}::uuid`
        return { username: String(profile.username), email: String(profile.email), userId: Number(profile.user_id), id }
    }
    const rename = (id: string, username: string) => sql`update user_profiles set username = ${username} where id = ${id}::uuid`
    try {
        await sql`create table auth.users (id uuid primary key, email text, raw_user_meta_data jsonb)`
        await sql`create trigger on_auth_user_created after insert on auth.users for each row execute function public.handle_new_user()`

        await t.test('email and Google sign ups both start as Guest-<user_id>', async () => {
            const google = await signUp({ full_name: 'Somebody' })
            const email = await signUp({ username: 'PickedName' })
            assert.equal(google.username, `Guest-${google.userId}`)
            assert.equal(email.username, `Guest-${email.userId}`)
            assert.equal(email.email, `${email.id}@example.com`)
        })

        await t.test('a name sent with the sign up cannot claim a Guest name either', async () => {
            const [{ nextUserId }] = await sql`select last_value + 1 as "nextUserId" from user_profiles_user_id_seq`
            const profile = await signUp({ username: `Guest-${Number(nextUserId) + 5}` })
            assert.equal(profile.username, `Guest-${profile.userId}`)
        })

        await t.test('an account can rename itself to a free name, but not to a taken one in any case', async () => {
            const first = await signUp({ full_name: 'First' })
            const second = await signUp({ full_name: 'Second' })
            await rename(first.id, 'NewName')
            await assert.rejects(rename(second.id, 'newname'), /user_profiles_username_lower_key/)
        })

        await t.test('no account can take another account\'s Guest name, or one for an account not made yet', async () => {
            const guest = await signUp({ full_name: 'Guest' })
            const other = await signUp({ full_name: 'Other' })
            await rename(other.id, 'Renamed')
            await assert.rejects(rename(other.id, `Guest-${guest.userId}`), /user_profiles_username_characters/)
            await assert.rejects(rename(other.id, `Guest-${guest.userId + 1000}`), /user_profiles_username_characters/)
            await assert.rejects(rename(other.id, `guest-${other.userId}`), /user_profiles_username_characters/)
        })
    } finally {
        await sql.end()
        await database.stop()
    }
})
