import { test } from 'node:test'
import assert from 'node:assert/strict'

// Exercise real handlers in memory. An unavailable database exercises actual driver failures without mocks.
Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'this-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: '8080',
    EDITOR_CHAT_PORT: '8081',
    SITE_URL: 'https://website.example',
    SUPABASE_URL: 'https://supabase.invalid',
    SUPABASE_SECRET_KEY: 'test-only',
    S3_ACCESS_KEY_ID: 'test-only',
    S3_SECRET_ACCESS_KEY: 'test-only',
    AWS_REGION: 'us-east-1',
    AWS_BEARER_TOKEN_BEDROCK: 'test-only',
    AZURE_OPENAI_API_KEY: 'test-only',
})
const { app } = await import('../src/endpoints.ts')
const { editorRuntime, serverToken } = await import('../src/helpers/worker.ts')
const { chat, chatEvents, interruptChat } = await import('../src/helpers/chat.ts')
const { database } = await import('../src/helpers/database.ts')
test('Node leaves world and playtest operations in Rust', async () => {
    for (const [method, path] of [
        ['GET', '/get-editor-state'],
        ['GET', '/internal/world'],
        ['PUT', '/internal/world'],
        ['POST', '/internal/scripts'],
        ['POST', '/playtest/start'],
        ['POST', '/playtest/stop'],
        ['POST', '/playtest/add-player'],
    ])
        assert.equal((await app.request(path, { method })).status, 404, `${method} ${path}`)
})
test('old routes are unavailable, even with the deployment credential', async () => {
    for (const path of ['/chat', '/chat-state', '/read-script-files', '/read-script-diagnostics']) {
        for (const token of ['', 'browser-access-token', serverToken]) {
            const response = await app.request(path, {
                method: 'POST',
                headers: token ? { Authorization: `Bearer ${token}` } : {},
            })
            assert.equal(response.status, 404)
        }
    }
})
test('local-only operations reject browser tokens and credentials from another editor', async () => {
    const { createHmac } = await import('node:crypto')
    const otherToken = createHmac('sha256', process.env.EDITOR_SERVER_SECRET!)
        .update('another-deployment')
        .digest('hex')
    for (const token of ['', 'browser-access-token', otherToken]) {
        const response = await app.request('/local-only/read-script-diagnostics', {
            method: 'POST',
            headers: token ? { Authorization: `Bearer ${token}` } : {},
        })
        assert.equal(response.status, 401)
    }
    for (const path of ['/local-only/read-script-diagnostics']) {
        for (const token of ['game-invitation', serverToken]) {
            const response = await app.request(path, {
                method: 'POST',
                headers: { Authorization: `Invite ${token}` },
            })
            assert.equal(response.status, 400)
        }
    }
})
test('settings no longer expose Node endpoints, even with the deployment credential', async () => {
    for (const path of ['/local-only/get_game_settings', '/local-only/set_game_settings']) {
        const response = await app.request(path, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${serverToken}` },
            body: JSON.stringify({ title: 'My game' }),
        })
        assert.equal(response.status, 404)
    }
})
test('Rust receives only diagnostics through the authenticated local-only route', async () => {
    const response = await app.request('/local-only/read-script-diagnostics', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${serverToken}` },
        body: JSON.stringify([
            { objectId: 2, path: 'root/main', text: 'const x: number = "bad";' },
            { objectId: 3, path: 'root/clean', text: 'const x = 1;' },
        ]),
    })
    assert.equal(response.status, 200)
    const diagnostics = await response.json()
    assert.deepEqual(Object.keys(diagnostics), ['2', '3'])
    assert.match(diagnostics[2][0].message, /not assignable/)
    assert.deepEqual(diagnostics[3], [])
    assert.equal('text' in diagnostics[2][0], false)
})
test('browser chat requires authentication and never trusts a supplied profile_id', async () => {
    for (const [method, path] of [
        ['GET', '/editor-server-chat-stream'],
        ['POST', '/chat-message'],
        ['POST', '/chat-interrupt'],
        ['POST', '/script-diagnostics'],
        ['GET', '/get-script-types'],
    ]) {
        const response = await app.request(path, {
            method,
            ...(method === 'POST'
                ? {
                      headers: { 'Content-Type': 'application/json' },
                      body: JSON.stringify({ profile_id: 'owner', body: { message: {} } }),
                  }
                : {}),
        })
        assert.equal(response.status, 401)
    }
})

test('malformed credentials cannot read chat or start work', async () => {
    for (const authorization of ['', 'Invite', 'Invite ', 'Bearer', 'Bearer ', 'Basic token']) {
        for (const [method, path] of [
            ['GET', '/editor-server-chat-stream'],
            ['POST', '/chat-message'],
        ]) {
            const response = await app.request(path, {
                method,
                headers: { Authorization: authorization },
            })
            assert.equal(response.status, 401)
        }
    }
})

test('invitations fail closed when their current database value cannot be verified', async () => {
    // No Supabase request is needed for guests. The real, unavailable database must prevent access.
    const response = await app.request('/get-script-types', {
        headers: { Authorization: 'Invite game-invitation' },
    })
    assert.equal(response.status, 500)
    assert.deepEqual(await response.json(), { error: 'The editor request failed.' })
})
test('direct browser chat supports authenticated CORS preflight', async () => {
    const response = await app.request('/chat-message', {
        method: 'OPTIONS',
        headers: {
            Origin: 'https://website.example',
            'Access-Control-Request-Method': 'POST',
            'Access-Control-Request-Headers': 'authorization,content-type',
        },
    })
    assert.equal(response.status, 204)
    assert.equal(response.headers.get('access-control-allow-origin'), 'https://website.example')
    assert.match(response.headers.get('access-control-allow-headers') ?? '', /Authorization/i)
})
test('malformed chat messages fail before opening a database connection', async () => {
    await assert.rejects(chat('profile', { message: { role: 'assistant' } }))
})
test('shutdown rejects new work', async () => {
    editorRuntime.isStopping = true
    try {
        assert.equal((await app.request('/chat-message', { method: 'POST' })).status, 503)
    } finally {
        editorRuntime.isStopping = false
    }
})
test('simultaneous chat requests are rejected during startup and failed startup releases the turn', async () => {
    // The credit check is the first thing a chat waits on, so failing it fails startup.
    const creditsUsedToday = database.creditsUsedToday
    database.creditsUsedToday = async () => {
        throw new Error('The database is unreachable.')
    }
    const reader = chatEvents
        .subscribe(async () => ({ type: 'state', messages: [] }), new AbortController().signal)
        .getReader()
    await reader.read()
    const body = {
        message: {
            id: 'startup-message',
            role: 'user',
            parts: [{ type: 'text', text: 'Build a game' }],
        },
    }
    const failedStartup = assert.rejects(chat('profile', body), {
        message: 'The database is unreachable.',
    })
    const rejectedSecondRequest = assert.rejects(chat('profile', body), { status: 409 })
    await Promise.all([failedStartup, rejectedSecondRequest, interruptChat()])
    assert.equal(
        (await reader.read()).done,
        true,
        'A failed startup must also finish existing subscriptions',
    )
    await assert.rejects(chat('profile', body), { message: 'The database is unreachable.' })
    database.creditsUsedToday = creditsUsedToday
})
