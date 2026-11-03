import { createMcpHandler, Server, type CallToolResult, type ImageContent } from '@modelcontextprotocol/server'
import { z } from 'zod'
import { MCP_ONLY_TOOLS } from './mcpTools'
import { sql } from '@/lib/db'
import { openDevEditorServer } from '@/lib/devEditorServer'
import { getEditorAccess } from '@/lib/editorAccess'
import { getEditorServerToken, openEditorServer } from '@/lib/editorServers'

// The MCP server: an AI outside kube.new (Codex, Claude Code, ChatGPT...) connects here with the game's invite,
// and calls the editor's tools. The editor server runs them the way it runs the built-in AI's, through the same
// /local-only routes Node uses. This route is what makes an outside AI different: a note in front of the editor's
// instructions, and tools of its own (MCP_ONLY_TOOLS).

export const runtime = 'nodejs'
// TODO: AI-PICKED-VALUE: Five minutes covers an editor's cold start, and a tool call that waits behind the built-in AI's.
export const maxDuration = 300

const isDev = process.env.NODE_ENV === 'development'

// -------------------------- Reaching the editor --------------------------

type RunningEditor = { request_id: string; editor_url: string }

// Opens the game's editor the same way the editor page does, waiting while it starts.
const openRunningEditor = async (gameId: number, forwardedFor: string | null, signal: AbortSignal): Promise<RunningEditor> => {
    while (true) {
        const opened = isDev
            ? await openDevEditorServer(gameId)
            : await openEditorServer(gameId, z.ipv4().or(z.ipv6()).parse(forwardedFor?.split(',')[0]?.trim()))
        if (opened.status === 'running') return opened
        // TODO: AI-PICKED-VALUE: Check on a starting editor every two seconds, like the editor page.
        await new Promise(resolve => setTimeout(resolve, 2000))
        signal.throwIfAborted()
    }
}

// The editor on record is asked first, so a call does not ask Edgegap where the editor is. When there is none, or it
// does not answer, the editor is opened the way the editor page opens it, which starts a new one when needed, and the
// request is sent once more. Dev runs one local editor and never uses the hosted tables.
const sendToRunningEditor = async ({ gameId, forwardedFor, signal, send }: { gameId: number, forwardedFor: string | null, signal: AbortSignal, send: (editor: RunningEditor) => Promise<Response> }) => {
    const recorded = isDev ? undefined : (await sql<RunningEditor[]>`select request_id, editor_url from running_editor_servers where game_id=${gameId}`).at(0)
    const response = recorded === undefined ? undefined : await send(recorded).catch(() => { signal.throwIfAborted(); return undefined })
    if (recorded !== undefined && response?.ok) return { editor: recorded, response }
    const editor = await openRunningEditor(gameId, forwardedFor, signal)
    const retried = await send(editor)
    if (!retried.ok) throw new Error(`The editor could not answer (${retried.status}): ${await retried.text()}`)
    return { editor, response: retried }
}

const callEditorLocalOnlyRoute = async ({ gameId, forwardedFor, path, body, signal }: { gameId: number, forwardedFor: string | null, path: string, body: unknown, signal: AbortSignal }) => {
    const { response } = await sendToRunningEditor({ gameId, forwardedFor, signal, send: editor => fetch(new URL(path, editor.editor_url), {
        method: body === undefined ? 'GET' : 'POST',
        headers: { Authorization: `Bearer ${getEditorServerToken(editor.request_id)}`, 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal,
    }) })
    return response
}

const toolListSchema = z.object({
    system_prompt: z.string(),
    tools: z.array(z.object({ name: z.string(), description: z.string(), inputSchema: z.looseObject({ type: z.literal('object') }) })),
})
const toolResultSchema = z.object({ is_error: z.boolean(), text: z.string(), images: z.array(z.object({ media_type: z.string(), base64: z.string() })) })

// -------------------------- The MCP server --------------------------

// One stateless server per HTTP request. Only the tool list and tool calls reach the editor, so connecting is quick even
// while the editor sleeps.
const createServer = (gameId: number, forwardedFor: string | null, inviteToken: string) => {
    // TODO: AI-PICKED-VALUE: MCP needs a server version, and nothing reads it.
    const server = new Server({ name: 'kube.new', version: '1.0.0' }, {
        capabilities: { tools: {} },
        instructions: `Read the system prompt before anything else: it explains how this editor works.`,
    })
    const readTools = async (signal: AbortSignal) =>
        toolListSchema.parse(await (await callEditorLocalOnlyRoute({ gameId, forwardedFor, path: '/local-only/tools', body: undefined, signal })).json())
    const toolResult = (content: CallToolResult['content'], isError: boolean) => server.projectCallToolResult({ content, isError }, undefined)

    server.setRequestHandler('tools/list', async (_request, ctx) => {
        const { tools } = await readTools(ctx.mcpReq.signal)
        return { tools: [...MCP_ONLY_TOOLS.map(({ tool }) => tool), ...tools] }
    })
    // Errors come back as failed tool results, so the AI can read why, such as the editor failing to start.
    server.setRequestHandler('tools/call', async (request, ctx) => {
        const { name } = request.params
        const args = request.params.arguments ?? {}
        try {
            const mcpOnlyTool = MCP_ONLY_TOOLS.find(({ tool }) => tool.name === name)
            if (mcpOnlyTool) {
                const text = await mcpOnlyTool.run(args, {
                    inviteToken,
                    readSystemPrompt: async () => (await readTools(ctx.mcpReq.signal)).system_prompt,
                    // The file tools give the AI the editor's address, so the editor is checked to answer first.
                    readEditorUrl: async () => (await sendToRunningEditor({ gameId, forwardedFor, signal: ctx.mcpReq.signal, send: editor => fetch(`${editor.editor_url}/health`, { headers: { Authorization: `Bearer ${getEditorServerToken(editor.request_id)}` }, signal: ctx.mcpReq.signal }) })).editor.editor_url,
                })
                return toolResult([{ type: 'text', text }], false)
            }
            const response = await callEditorLocalOnlyRoute({ gameId, forwardedFor, path: '/local-only/run-tool-call', body: { name, arguments: args }, signal: ctx.mcpReq.signal })
            const result = toolResultSchema.parse(await response.json())
            const images = result.images.map((image): ImageContent => ({ type: 'image', data: image.base64, mimeType: image.media_type }))
            return toolResult([{ type: 'text', text: result.text }, ...images], result.is_error)
        } catch (error) {
            return toolResult([{ type: 'text', text: error instanceof Error ? error.message : String(error) }], true)
        }
    })
    return server
}

// The link carries the game's invite, the same one people use, so turning sharing off turns this off too.
export const POST = async (request: Request, { params }: { params: Promise<{ gameId: string }> }) => {
    const gameId = z.coerce.number().int().positive().safe().safeParse((await params).gameId)
    const inviteToken = new URL(request.url).searchParams.get('invite')
    if (!gameId.success || !inviteToken || !(await getEditorAccess(gameId.data, null, inviteToken)))
        return Response.json({ error: 'This MCP link is wrong, or its game stopped sharing.' }, { status: 403 })
    const forwardedFor = request.headers.get('x-forwarded-for')
    return createMcpHandler(() => createServer(gameId.data, forwardedFor, inviteToken)).fetch(request)
}
