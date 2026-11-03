import { HTTPException } from 'hono/http-exception'
import { Agent, FormData, fetch } from 'undici'
import { env } from './env.ts'
import { createHmac } from 'node:crypto'

export const serverToken = createHmac('sha256', env.EDITOR_SERVER_SECRET)
    .update(env.ARBITRIUM_REQUEST_ID)
    .digest('hex')

export const editorRuntime = { isStopping: false }

// A tool replies only when it finishes, and it can wait behind other tools for longer than fetch's default five minute
// timeouts.
const dispatcherWithoutTimeouts = new Agent({ headersTimeout: 0, bodyTimeout: 0 })

// Local-only tool routes require the deployment credential; browser credentials cannot call them. A file is sent as it
// is, and anything else as JSON.
export const workerFetch = async (
    path: string,
    method: string,
    body: unknown,
    signal: AbortSignal | undefined,
) => {
    const fileBody = body instanceof Uint8Array || body instanceof FormData ? body : undefined
    const response = await fetch(`http://127.0.0.1:${env.EDITOR_SERVER_PORT}${path}`, {
        method,
        headers: {
            Authorization: `Bearer ${serverToken}`,
            ...(fileBody ? {} : { 'Content-Type': 'application/json' }),
        },
        body: fileBody ?? (body === undefined ? undefined : JSON.stringify(body)),
        signal,
        dispatcher: dispatcherWithoutTimeouts,
    })
    if (!response.ok)
        throw new HTTPException(response.status === 409 ? 409 : 502, {
            message: await response.text(),
        })
    return response
}
