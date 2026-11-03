import { after, test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { setImmediate as flushCallbacks } from 'node:timers/promises'
import type { UIMessage } from 'ai'

// workerFetch sends to the editor's port, so a real local server stands in for the editor and records what arrives.
const requests: { contentType: string | undefined; body: Buffer }[] = []
const editor = createServer((request, response) => {
    const chunks: Buffer[] = []
    request.on('data', (chunk: Buffer) => chunks.push(chunk))
    request.on('end', () => {
        requests.push({ contentType: request.headers['content-type'], body: Buffer.concat(chunks) })
        response.setHeader('Content-Type', 'application/json').end('{}')
    })
})
await new Promise<void>((resolve) => editor.listen(0, '127.0.0.1', resolve))
const address = editor.address()
assert.ok(address !== null && typeof address === 'object')
Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'this-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: String(address.port),
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
const { callsAnyTool, createCallQueue, readSubagentActivity, TOOL_NAMES_SUBAGENTS_DONT_GET } =
    await import('../src/helpers/chatTools.ts')
const { workerFetch } = await import('../src/helpers/worker.ts')
const { FormData } = await import('undici')
after(async () => {
    editor.close()
})

test('calls run one at a time in order, except that subagent calls next to each other run together', async () => {
    const queueCall = createCallQueue()
    const events: string[] = []
    const finishOfName = {
        ls: Promise.withResolvers<void>(),
        subagentA: Promise.withResolvers<void>(),
        subagentB: Promise.withResolvers<void>(),
        edit: Promise.withResolvers<void>(),
    }
    const call = (name: keyof typeof finishOfName, isSubagentCall: boolean) =>
        queueCall(isSubagentCall, async () => {
            events.push(`start ${name}`)
            await finishOfName[name].promise
            events.push(`end ${name}`)
        })
    const calls = [
        call('ls', false),
        call('subagentA', true),
        call('subagentB', true),
        call('edit', false),
    ]
    await flushCallbacks()
    assert.deepEqual(events, ['start ls'])
    finishOfName.ls.resolve()
    await flushCallbacks()
    assert.deepEqual(events, ['start ls', 'end ls', 'start subagentA', 'start subagentB'])
    finishOfName.subagentB.resolve()
    await flushCallbacks()
    assert.deepEqual(
        events.slice(4),
        ['end subagentB'],
        'A call after the subagents waits for all of them',
    )
    finishOfName.subagentA.resolve()
    await flushCallbacks()
    assert.deepEqual(events.slice(5), ['end subagentA', 'start edit'])
    finishOfName.edit.resolve()
    await Promise.all(calls)
})

test('a failed call does not hold up the calls after it', async () => {
    const queueCall = createCallQueue()
    const failed = queueCall(false, async () => {
        throw new Error('The tool failed.')
    })
    await assert.rejects(failed, /The tool failed/)
    assert.equal(await queueCall(false, async () => 'next'), 'next')
    assert.equal(await queueCall(true, async () => 'subagent'), 'subagent')
})

// Subagents don't get some tools, and run_parallel can call any tool and can nest, so a call to one has to be found
// wherever run_parallel would find it.
test('a subagent call to a tool subagents do not get is found directly and inside nested run_parallel calls', () => {
    assert.equal(callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'publish', {}), true)
    assert.equal(callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'ls', {}), false)
    const nested = JSON.stringify({
        tool_calls: [{ tool_name: 'catalog_item_publish', args: JSON.stringify({ asset_id: 1 }) }],
    })
    assert.equal(
        callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'run_parallel', {
            tool_calls: [
                { tool_name: 'ls', args: '{}' },
                { tool_name: 'run_parallel', args: nested },
            ],
        }),
        true,
    )
    assert.equal(
        callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'run_parallel', {
            tool_calls: [{ tool_name: 'ls', args: '{}' }],
        }),
        false,
    )
})

test('run_parallel subcalls are read the way the editor reads them', () => {
    // The editor also accepts `tool` for the tool's name.
    assert.equal(
        callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'run_parallel', {
            tool_calls: [{ tool: 'publish', args: '{}' }],
        }),
        true,
    )
    // run_parallel refuses a subcall whose arguments are not a JSON string, and arguments it cannot read at all, so those
    // run nothing.
    assert.equal(
        callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'run_parallel', {
            tool_calls: [{ tool_name: 'publish', args: 'not json' }],
        }),
        false,
    )
    assert.equal(
        callsAnyTool(TOOL_NAMES_SUBAGENTS_DONT_GET, 'run_parallel', { tool_calls: 'publish' }),
        false,
    )
})

// While a subagent works, the chat shows the tool it is running and the last text it finished.
const makeSubagentReply = (parts: UIMessage['parts']): UIMessage => ({
    id: 'reply',
    role: 'assistant',
    parts,
})
test('a subagent is shown running its last tool until that tool returns, with the last text it finished', () => {
    const ranCommand: UIMessage['parts'][number] = {
        type: 'dynamic-tool',
        toolName: 'sandbox_run_command',
        toolCallId: 'call-1',
        state: 'output-available',
        input: { command: 'blender --background', timeout_seconds: 60 },
        output: { is_error: false, text: '{}', images: [] },
    }
    const viewingImage: UIMessage['parts'][number] = {
        type: 'dynamic-tool',
        toolName: 'sandbox_view_image',
        toolCallId: 'call-2',
        state: 'input-available',
        input: { path: 'ship/front.png' },
    }
    assert.deepEqual(
        readSubagentActivity(
            makeSubagentReply([
                { type: 'text', text: 'Building the hull first.', state: 'done' },
                ranCommand,
                { type: 'step-start' },
                {
                    type: 'text',
                    text: 'The hull renders cleanly. Checking the front view.',
                    state: 'done',
                },
                viewingImage,
            ]),
        ),
        {
            toolName: 'sandbox_view_image',
            latestText: 'The hull renders cleanly. Checking the front view.',
        },
    )
    assert.deepEqual(
        readSubagentActivity(
            makeSubagentReply([
                ranCommand,
                { type: 'text', text: 'Adding the mast', state: 'streaming' },
            ]),
        ),
        { toolName: null, latestText: '' },
        'A returned tool is no longer running, and text still streaming is not shown yet',
    )
})

test("a subagent running run_parallel is shown running that call's first tool", () => {
    const runParallel = (input: unknown): UIMessage['parts'][number] => ({
        type: 'dynamic-tool',
        toolName: 'run_parallel',
        toolCallId: 'call-1',
        state: 'input-available',
        input,
    })
    const ls = { tool_name: 'ls', args: '{}' }
    const toolNameOf = (input: unknown) =>
        readSubagentActivity(makeSubagentReply([runParallel(input)])).toolName
    assert.equal(
        toolNameOf({ tool_calls: [{ tool_name: 'create_brick', args: '{}' }, ls] }),
        'create_brick',
    )
    assert.equal(
        toolNameOf({ tool_calls: [{ tool: 'delete_object', args: '{}' }, ls] }),
        'delete_object',
    )
    // While the AI is still writing the call, its input can't be read yet.
    assert.equal(toolNameOf(undefined), 'run_parallel')
})

test("a subagent's latest text skips blank text and is cut short", () => {
    const { latestText } = readSubagentActivity(
        makeSubagentReply([
            { type: 'text', text: `  ${'a'.repeat(400)}  `, state: 'done' },
            { type: 'text', text: '   ', state: 'done' },
        ]),
    )
    assert.equal(latestText, 'a'.repeat(300))
})

// The import tools send a sandbox file to the editor's upload routes, which read the raw file or a multipart form.
test('a file reaches the editor as it is, a form as multipart, and anything else as JSON', async () => {
    const file = Buffer.from('glTF model bytes')
    await workerFetch('/local-only/asset-import', 'POST', file, undefined)
    assert.equal(requests.at(-1)?.contentType, undefined)
    assert.deepEqual(requests.at(-1)?.body, file)
    const form = new FormData()
    form.append('albedo', new Blob([Buffer.from('png bytes')]), 'bark.png')
    await workerFetch('/local-only/asset-import-material', 'POST', form, undefined)
    assert.match(requests.at(-1)?.contentType ?? '', /^multipart\/form-data; boundary=/)
    assert.match(
        requests.at(-1)?.body.toString() ?? '',
        /name="albedo"; filename="bark.png"[\s\S]*png bytes/,
    )
    await workerFetch('/local-only/run-tool-call', 'POST', { name: 'ls', arguments: {} }, undefined)
    assert.equal(requests.at(-1)?.contentType, 'application/json')
    assert.deepEqual(JSON.parse(requests.at(-1)?.body.toString() ?? ''), {
        name: 'ls',
        arguments: {},
    })
})
