import { Hono } from 'hono'
import { bodyLimit } from 'hono/body-limit'
import { bearerAuth } from 'hono/bearer-auth'
import { cors } from 'hono/cors'
import { HTTPException } from 'hono/http-exception'
import { z } from 'zod'
import { aiThumbnailRequestSchema, generateAiThumbnail } from './helpers/aiThumbnail.ts'
import { chat, chatEvents, getChatState, interruptChat } from './helpers/chat.ts'
import { requireEditorAccess } from './helpers/auth.ts'
import { env, isDev } from './helpers/env.ts'
import { editorRuntime, serverToken } from './helpers/worker.ts'
import {
    readScriptDiagnostics,
    scriptSnapshotSchema,
    scriptTypeEnvironment,
} from './helpers/scripts.ts'

// Browser chat and Rust-to-Node operations share this listener, with separate authentication.
export const app = new Hono()
app.use(
    '*',
    cors({
        origin: env.SITE_URL,
        allowHeaders: ['Authorization', 'Content-Type', 'x-editor-session'],
        allowMethods: ['GET', 'POST', 'OPTIONS'],
    }),
)
// TODO: AI-PICKED-VALUE: Allow full world payloads up to 64 MiB.
app.use('*', bodyLimit({ maxSize: 64 * 1024 * 1024 }))
app.onError((error, c) => {
    if (error instanceof HTTPException) return c.json({ error: error.message }, error.status)
    if (error instanceof z.ZodError)
        return c.json({ error: 'Invalid request.', details: error.issues }, 400)
    console.error(error)
    return c.json({ error: 'The editor request failed.' }, 500)
})
app.use('*', async (_c, next) => {
    if (editorRuntime.isStopping)
        throw new HTTPException(503, { message: 'The editor is shutting down.' })
    await next()
})
const browser = new Hono()
// Subscribe before reading history so simultaneous messages cannot fall between the two.
browser.get('/editor-server-chat-stream', requireEditorAccess, async (c) => {
    // TODO: AI-PICKED-VALUE: Renew subscriptions after at most fifteen minutes so idle streams also recheck access.
    const signal = AbortSignal.any([c.req.raw.signal, AbortSignal.timeout(15 * 60_000)])
    return new Response(chatEvents.subscribe(getChatState, signal), {
        headers: {
            'Content-Type': 'application/x-ndjson',
            'Cache-Control': 'no-store',
            'X-Accel-Buffering': 'no',
        },
    })
})
browser.post('/chat-message', requireEditorAccess, async (c) => chat(c.get('profileId'), await c.req.json()))
browser.post('/chat-interrupt', requireEditorAccess, async (c) => {
    await interruptChat()
    return c.body(null, 204)
})
// browser asks the server for script diagnostics
browser.post('/script-diagnostics', requireEditorAccess, async (c) =>
    c.json(readScriptDiagnostics(scriptSnapshotSchema.parse(await c.req.json()))),
)
browser.get('/get-script-types', requireEditorAccess, (c) => c.json(scriptTypeEnvironment))
// Match Rust's dev URL prefix so stale browser tabs cannot chat with a replacement game.
app.route(isDev ? `/${env.ARBITRIUM_REQUEST_ID}` : '/', browser)

// These operations are called inside the container. The deployment token protects them, not the URL prefix.
app.use('/local-only/*', bearerAuth({ token: serverToken }))
app.post('/local-only/read-script-diagnostics', async (c) =>
    c.json(readScriptDiagnostics(scriptSnapshotSchema.parse(await c.req.json()))),
)
app.post('/local-only/generate-ai-thumbnail', async (c) =>
    c.body(await generateAiThumbnail(aiThumbnailRequestSchema.parse(await c.req.json())), 200, {
        'Content-Type': 'image/png',
    }),
)
