import { createHmac } from 'node:crypto'
import { z } from 'zod'
import { sql } from '@/lib/db'
import {
    getEdgegapAuthorizationHeader,
    getRequiredEnvVar,
    getSecretEnvVars,
    type EdgegapWebhookLifecycleEvent,
} from '@/lib/edgegapApi'
import {
    editorDeploymentStatusSchema,
    readEditorConnectionInfo,
} from '@/lib/editorServerConnection'

// The website owns editor allocation and tracking; each container owns its world and shutdown.
export class EditorServerError extends Error {
    status: number
    constructor(status: number, message: string) {
        super(message)
        this.status = status
    }
}

export const getEditorServerToken = (requestId: string) =>
    createHmac('sha256', getRequiredEnvVar('EDITOR_SERVER_SECRET')).update(requestId).digest('hex')

const getEditorWebhookUrl = (event: EdgegapWebhookLifecycleEvent) => {
    const baseUrl = getRequiredEnvVar('EDGEGAP_WEBHOOK_BASE_URL').trim().replace(/\/+$/, '')
    const secret = encodeURIComponent(getRequiredEnvVar('EDGEGAP_WEBHOOK_SECRET'))
    return `${baseUrl}/api/edgegap-editor/webhooks/${event}?secret=${secret}`
}

const edgegap = async (path: string, method: string, body: unknown) => {
    for (let attempt = 0; ; attempt++) {
        const response = await fetch(`https://api.edgegap.com${path}`, {
            method,
            headers: {
                Authorization: getEdgegapAuthorizationHeader(),
                'Content-Type': 'application/json',
            },
            body: body === undefined ? undefined : JSON.stringify(body),
            signal: AbortSignal.timeout(30_000),
        })
        // TODO: AI-PICKED-VALUE: A rate-limited request is tried twice more, after 1 and 2 seconds.
        if (response.status === 429 && attempt < 2) {
            await new Promise((resolve) => setTimeout(resolve, 1000 * 2 ** attempt))
            continue
        }
        if (response.status === 404) return null
        if (!response.ok) throw new Error(`Edgegap request failed (${response.status}).`)
        return response
    }
}
// Promotion and cleanup lock the same game row as allocation. Cleanup cannot remove a replacement.
export const deleteEditorServer = async (requestId: string) =>
    sql.begin(async (transaction) => {
        const [tracked] = await transaction<
            { game_id: number }[]
        >`select game_id from starting_editor_servers where request_id=${requestId} union all select game_id from running_editor_servers where request_id=${requestId}`
        if (!tracked) return
        await transaction`select game_id from game_metadata where game_id=${tracked.game_id} for update`
        await transaction`delete from starting_editor_servers where request_id=${requestId}`
        await transaction`delete from running_editor_servers where request_id=${requestId}`
    })
export const reconcileEditorServer = async (requestId: string) => {
    // TODO: AI-PICKED-VALUE: A worker that cannot become ready within five minutes is a failed startup.
    const [starting] = await sql<
        { game_id: number; is_expired: boolean; is_recent: boolean }[]
    >`select game_id, starting_at < now() - interval '5 minutes' as is_expired, starting_at > now() - interval '2 minutes' as is_recent from starting_editor_servers where request_id=${requestId}`
    if (starting?.is_expired) {
        await stopEditorServer(requestId)
        return
    }
    const response = await edgegap(`/v1/status/${encodeURIComponent(requestId)}`, 'GET', undefined)
    if (!response) {
        if (!starting?.is_recent) await deleteEditorServer(requestId)
        return
    }
    const status = editorDeploymentStatusSchema.parse(await response.json())
    if (
        status.error ||
        /error|failed|terminated|stopped|deleted|removed/i.test(status.current_status)
    ) {
        await deleteEditorServer(requestId)
        return
    }
    const connection = readEditorConnectionInfo(status)
    if (!connection) return
    const {
        editor_url: editorUrl,
        public_ip: publicIp,
        playtest_external_port: playtestPort,
    } = connection
    // Edgegap routing can be ready before the world has finished loading.
    // An editor that is stopping fails this too, until it has saved and removed its own row.
    try {
        const health = await fetch(`${editorUrl}/health`, {
            headers: { Authorization: `Bearer ${getEditorServerToken(requestId)}` },
            signal: AbortSignal.timeout(3000),
        })
        if (!health.ok) return
    } catch {
        return
    }
    // Promotion belongs to startup.
    if (!starting) return connection
    await sql.begin(async (transaction) => {
        await transaction`select game_id from game_metadata where game_id=${starting.game_id} for update`
        const [current] =
            await transaction`select request_id from starting_editor_servers where request_id=${requestId}`
        if (!current) return
        await transaction`insert into running_editor_servers(request_id,game_id,editor_url,public_ip,playtest_external_port) values(${requestId},${starting.game_id},${editorUrl},${publicIp},${playtestPort})`
        await transaction`delete from starting_editor_servers where request_id=${requestId}`
    })
    return connection
}

// What opening an editor answers: it is still starting, or it is running at these addresses.
export type OpenedEditor = { status: 'starting' } | { status: 'running'; request_id: string; editor_url: string; chat_url: string }

export const openEditorServer = async (gameId: number, requesterIp: string): Promise<OpenedEditor> => {
    const application = getRequiredEnvVar('EDGEGAP_EDITOR_APPLICATION_NAME')
    const version = getRequiredEnvVar('EDGEGAP_EDITOR_VERSION_NAME')
    const tracked = await sql<
        { request_id: string; status: string }[]
    >`select request_id, 'starting' as status from starting_editor_servers where game_id=${gameId} union all select request_id, 'running' as status from running_editor_servers where game_id=${gameId}`
    let connection: Awaited<ReturnType<typeof reconcileEditorServer>>
    for (const server of tracked) {
        connection = await reconcileEditorServer(server.request_id)
        const [current] =
            await sql`select request_id from starting_editor_servers where request_id=${server.request_id} union all select request_id from running_editor_servers where request_id=${server.request_id}`
        if (server.status === 'starting' && !current)
            throw new EditorServerError(503, 'The editor could not start. Try again.')
    }
    let createdRequestId: string | undefined
    try {
        return await sql.begin(async (transaction): Promise<OpenedEditor> => {
            await transaction`select game_id from game_metadata where game_id=${gameId} for update`
            const [running] = await transaction<
                { request_id: string }[]
            >`select request_id from running_editor_servers where game_id=${gameId}`
            if (running) {
                if (connection?.request_id !== running.request_id) return { status: 'starting' }
                return {
                    status: 'running',
                    request_id: connection.request_id,
                    editor_url: connection.editor_url,
                    chat_url: connection.chat_url,
                }
            }
            const [starting] =
                await transaction`select request_id from starting_editor_servers where game_id=${gameId}`
            if (starting) return { status: 'starting' }
            const response = await edgegap('/v2/deployments', 'POST', {
                application,
                version,
                users: [{ user_type: 'ip_address', user_data: { ip_address: requesterIp } }],
                webhook_on_ready: { url: getEditorWebhookUrl('ready') },
                webhook_on_error: { url: getEditorWebhookUrl('error') },
                webhook_on_terminated: { url: getEditorWebhookUrl('terminated') },
                environment_variables: [
                    { key: 'GAME_ID', value: String(gameId), is_hidden: false },
                    ...getSecretEnvVars('editorServer'),
                ],
                tags: [`g=${gameId}`],
            })
            if (!response) throw new Error('Editor Edgegap application was not found.')
            const deployment = z.object({ request_id: z.string() }).parse(await response.json())
            createdRequestId = deployment.request_id
            console.log(`Started editor ${deployment.request_id} for game ${gameId}`)
            await transaction`insert into starting_editor_servers(request_id,game_id) values(${deployment.request_id},${gameId})`
            return { status: 'starting' }
        })
    } catch (error) {
        if (createdRequestId) await stopEditorServer(createdRequestId)
        throw error
    }
}
const stopEditorServer = async (requestId: string) => {
    await edgegap(`/v1/stop/${encodeURIComponent(requestId)}`, 'DELETE', undefined)
    await deleteEditorServer(requestId)
}
