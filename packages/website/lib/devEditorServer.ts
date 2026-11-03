import { spawn, type ChildProcess } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { resolve } from 'node:path'
import { EditorServerError, getEditorServerToken, type OpenedEditor } from '@/lib/editorServers'

type DevEditorState = {
    editor:
        | {
              gameId: number
              requestId: string
              url: string
              process: ChildProcess
              closed: Promise<void>
          }
        | undefined
    startingGameId: number | undefined
}
declare global {
    var devEditorState: DevEditorState | undefined
}

// Keep the one dev editor across Next.js hot reloads. Hosted deployment tables are not involved.
const state = (globalThis.devEditorState ??= { editor: undefined, startingGameId: undefined })

const stopDevEditorServer = async () => {
    const current = state.editor
    if (!current) return
    current.process.kill('SIGTERM')
    await current.closed
    if (state.editor === current) state.editor = undefined
}

export const openDevEditorServer = async (gameId: number): Promise<OpenedEditor> => {
    if (state.startingGameId !== undefined) {
        if (state.startingGameId !== gameId)
            throw new EditorServerError(409, 'Another dev editor is starting.')
        return { status: 'starting' }
    }
    if (
        state.editor &&
        (state.editor.process.exitCode !== null || state.editor.process.signalCode !== null)
    ) {
        const failed = state.editor.process.exitCode !== 0
        await state.editor.closed
        state.editor = undefined
        if (failed)
            throw new EditorServerError(
                503,
                'The dev editor exited. See the terminal for the error, then try again.',
            )
    }
    if (state.editor?.gameId !== gameId) {
        state.startingGameId = gameId
        try {
            await stopDevEditorServer()
            const requestId = randomUUID()
            // TODO: AI-PICKED-VALUE: Use 8080 for Rust, 8081 for Node chat, and 5000 for desktop playtests.
            const url = `http://127.0.0.1:8080/${requestId}`
            const child = spawn('editor_server', ['--dev'], {
                cwd: resolve(process.cwd(), '../../crates/editor_server'),
                // Rust saves and exits when Next.js exits and this stdin pipe closes.
                stdio: ['pipe', 'inherit', 'inherit'],
                env: {
                    ...process.env,
                    EDITOR_SERVER_PORT: '8080',
                    EDITOR_CHAT_PORT: '8081',
                    EDITOR_PLAYTEST_PORT: '5000',
                    GAME_ID: String(gameId),
                    ARBITRIUM_REQUEST_ID: requestId,
                },
            })
            child.on('error', (error) => console.error('Could not start dev editor', error))
            const closed = new Promise<void>((resolve) => child.once('close', () => resolve()))
            state.editor = { gameId, requestId, url, process: child, closed }
        } finally {
            state.startingGameId = undefined
        }
    }
    const current = state.editor
    if (!current) throw new EditorServerError(503, 'The dev editor did not start.')
    try {
        const response = await fetch(`${current.url}/health`, {
            headers: { Authorization: `Bearer ${getEditorServerToken(current.requestId)}` },
            signal: AbortSignal.timeout(3000),
        })
        if (!response.ok) return { status: 'starting' }
    } catch {
        return { status: 'starting' }
    }
    return {
        status: 'running',
        request_id: current.requestId,
        editor_url: current.url,
        chat_url: `http://127.0.0.1:8081/${current.requestId}`,
    }
}
