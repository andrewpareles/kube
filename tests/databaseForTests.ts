import { execFile, spawn } from 'node:child_process'
import { createHmac, randomBytes } from 'node:crypto'
import { once } from 'node:events'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { createServer as createHttpServer, request as httpRequest } from 'node:http'
import { connect, createServer } from 'node:net'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { promisify } from 'node:util'
import { repoPath } from './testPaths.ts'

const run = promisify(execFile)
// Postgres on macOS refuses to start without a valid locale in the environment.
const serverEnvironment = { ...process.env, LC_ALL: 'C' }

// Supabase provides these roles, auth.uid() and storage.buckets, which production's schema and migrations refer to; the rest comes from the dump.
const SUPABASE_STAND_IN = `
create role supabase_admin nologin;
create role anon nologin;
create role authenticated nologin;
create role service_role nologin bypassrls;
create schema auth;
create function auth.uid() returns uuid language sql stable as $$
    select coalesce(nullif(current_setting('request.jwt.claim.sub', true), ''), (nullif(current_setting('request.jwt.claims', true), '')::jsonb ->> 'sub'))::uuid
$$;
create schema storage;
create table storage.buckets (id text primary key, name text not null, public boolean not null default false);
drop schema public cascade;
`

export const findFreePort = () => new Promise<number>((resolve, reject) => {
    const server = createServer()
    server.on('error', reject)
    server.listen(0, 'localhost', () => {
        const address = server.address()
        server.close(() => typeof address === 'object' && address !== null ? resolve(address.port) : reject(new Error('No port was assigned')))
    })
})

// The dump names the last migration production had. Every later migration in the journal is applied on top, so a test
// sees the schema this code runs against once it deploys.
const readPendingMigrationPaths = async (schemaSql: string) => {
    const lastMigrationTag = /after migration (\S+),/.exec(schemaSql)?.[1]
    const journal: { entries: { tag: string }[] } = JSON.parse(await readFile(repoPath('packages/website/supabase/migrations/meta/_journal.json'), 'utf8'))
    const tags = journal.entries.map(entry => entry.tag)
    const lastMigrationIndex = tags.findIndex(tag => tag === lastMigrationTag)
    if (lastMigrationIndex === -1) throw new Error(`The schema dump names migration ${lastMigrationTag}, which is not in the journal`)
    return tags.slice(lastMigrationIndex + 1).map(tag => repoPath(`packages/website/supabase/migrations/${tag}.sql`))
}

// A throwaway Postgres 17 in UTC, like production, in a temporary directory that stop() deletes. It only listens on localhost.
export const startDatabaseForTests = async (port: number) => {
    const { stdout: version } = await run('postgres', ['--version'])
    if (!/ 17\./.test(version)) throw new Error(`Database tests need Postgres 17 first on PATH, like production, but found ${version.trim()}. With Homebrew: PATH=/opt/homebrew/opt/postgresql@17/bin:$PATH`)
    const directory = await mkdtemp(path.join(tmpdir(), 'database-for-tests-'))
    const dataDirectory = path.join(directory, 'data')
    await run('initdb', ['-D', dataDirectory, '-U', 'postgres', '-A', 'trust', '-E', 'UTF8', '--locale=C', '--no-sync', '--no-instructions'], { env: serverEnvironment })
    await run('pg_ctl', ['-D', dataDirectory, '-l', path.join(directory, 'server.log'), '-w', '-o', `-p ${port} -c listen_addresses=localhost -c unix_socket_directories= -c fsync=off -c timezone=UTC`, 'start'], { env: serverEnvironment })
    const stop = async () => {
        await run('pg_ctl', ['-D', dataDirectory, '-m', 'immediate', 'stop'], { env: serverEnvironment })
        await rm(directory, { recursive: true, force: true })
    }
    try {
        const psql = (args: string[]) => run('psql', ['-X', '-q', '-v', 'ON_ERROR_STOP=1', '-h', 'localhost', '-p', String(port), '-U', 'postgres', '-d', 'postgres', ...args])
        const schemaPath = repoPath('tests/fixtures/database_schema.sql')
        await psql(['-c', SUPABASE_STAND_IN])
        await psql(['-f', schemaPath])
        for (const migrationPath of await readPendingMigrationPaths(await readFile(schemaPath, 'utf8'))) await psql(['-f', migrationPath])
    } catch (error) {
        await stop()
        throw error
    }
    return { url: `postgres://postgres@localhost:${port}/postgres`, stop }
}

// The website's database module always connects to the port of Supabase's transaction pooler. Test files run at the same time,
// so the ones that run website code take turns on that port: each holds it until stop() and passes its connections on to its
// own database. Holding the port does not keep the test process running, so a test that fails before stop() still ends.
const POOLER_PORT = 6543
export const startDatabaseOnPoolerPortForTests = async () => {
    const port = await findFreePort()
    const database = await startDatabaseForTests(port)
    const pooler = createServer(client => {
        const server = connect(port, 'localhost')
        client.on('error', () => server.destroy())
        server.on('error', () => client.destroy())
        client.pipe(server).pipe(client)
    }).unref()
    while (!pooler.listening) {
        try {
            await once(pooler.listen(POOLER_PORT, '127.0.0.1'), 'listening')
        } catch (error) {
            if (!(error instanceof Error && 'code' in error && error.code === 'EADDRINUSE')) {
                await database.stop()
                throw error
            }
            await new Promise(resolve => setTimeout(resolve, 500))
        }
    }
    const stop = async () => {
        pooler.close()
        await database.stop()
    }
    return { url: `postgres://postgres@127.0.0.1:${POOLER_PORT}/postgres`, stop }
}

// PostgREST over a database from startDatabaseForTests, at /rest/v1 behind a stand-in for Supabase's gateway, which turns away
// a request whose apikey header isn't the service key. The key is a JWT for service_role, which PostgREST checks.
export const startPostgrestForTests = async (databaseUrl: string) => {
    try {
        await run('postgrest', ['--version'])
    } catch {
        throw new Error('Database tests need PostgREST on PATH. With Homebrew: brew install postgrest')
    }
    const jwtSecret = randomBytes(32).toString('hex')
    const encode = (part: object) => Buffer.from(JSON.stringify(part)).toString('base64url')
    const unsignedKey = `${encode({ alg: 'HS256', typ: 'JWT' })}.${encode({ role: 'service_role' })}`
    const serviceKey = `${unsignedKey}.${createHmac('sha256', jwtSecret).update(unsignedKey).digest('base64url')}`
    const [postgrestPort, adminPort] = [await findFreePort(), await findFreePort()]
    const postgrest = spawn('postgrest', [], {
        env: {
            ...process.env,
            PGRST_DB_URI: databaseUrl,
            PGRST_DB_SCHEMAS: 'public',
            PGRST_DB_ANON_ROLE: 'anon',
            PGRST_JWT_SECRET: jwtSecret,
            PGRST_SERVER_HOST: '127.0.0.1',
            PGRST_SERVER_PORT: String(postgrestPort),
            PGRST_ADMIN_SERVER_PORT: String(adminPort),
        },
        stdio: ['ignore', 'pipe', 'pipe'],
    })
    let output = ''
    postgrest.stdout.on('data', chunk => { output += chunk })
    postgrest.stderr.on('data', chunk => { output += chunk })
    const exited = once(postgrest, 'exit')
    const gateway = createHttpServer((request, response) => {
        if (request.headers.apikey !== serviceKey) {
            response.writeHead(401, { 'content-type': 'application/json' }).end(JSON.stringify({ message: 'Invalid API key' }))
            return
        }
        if (!request.url?.startsWith('/rest/v1/')) {
            response.writeHead(404).end()
            return
        }
        const forwarded = httpRequest({ host: '127.0.0.1', port: postgrestPort, method: request.method, path: request.url.slice('/rest/v1'.length), headers: request.headers }, answer => {
            response.writeHead(answer.statusCode ?? 502, answer.headers)
            answer.pipe(response)
        })
        forwarded.on('error', error => response.destroy(error))
        request.pipe(forwarded)
    })
    const stop = async () => {
        gateway.closeAllConnections()
        gateway.close()
        if (postgrest.exitCode === null) {
            postgrest.kill()
            await exited
        }
    }
    try {
        const deadline = Date.now() + 30_000
        while ((await fetch(`http://127.0.0.1:${adminPort}/ready`).then(response => response.status, () => undefined)) !== 200) {
            if (postgrest.exitCode !== null) throw new Error(`PostgREST stopped while starting:\n${output}`)
            if (Date.now() > deadline) throw new Error(`PostgREST was not ready after 30 seconds:\n${output}`)
            await new Promise(resolve => setTimeout(resolve, 100))
        }
        gateway.listen(0, '127.0.0.1')
        await once(gateway, 'listening')
    } catch (error) {
        await stop()
        throw error
    }
    const address = gateway.address()
    if (typeof address !== 'object' || address === null) throw new Error('The gateway has no port')
    return { url: `http://127.0.0.1:${address.port}`, serviceKey, stop }
}
