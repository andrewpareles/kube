import { spawn, execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { createInterface } from 'node:readline'
import { promisify } from 'node:util'
import { z } from 'zod'

const replySchema = z.union([
    z.object({ id: z.number(), result: z.unknown() }),
    z.object({ id: z.number(), error: z.string() }),
])

// One JSON-line connection to a credential-free worker. Requests run at the same time in its workspace, and each
// reply repeats its request's id.
export const openLinuxSandbox = () => {
    const controller = new AbortController()
    const container =
        process.platform === 'linux' ? undefined : `kube-linux-sandbox-${randomUUID()}`
    const child = container
        ? spawn('docker', [
              'run',
              '--rm',
              '--interactive',
              '--init',
              '--name',
              container,
              '--network=none',
              '--read-only',
              '--security-opt=no-new-privileges',
              '--memory=3g',
              '--memory-swap=3g',
              '--cpus=1',
              '--pids-limit=128',
              '--shm-size=64m',
              'kube-editor-linux-sandbox',
          ])
        : spawn('linux_sandbox_worker', [], { env: { PATH: process.env.PATH } })
    const exited = new Promise<void>((resolve) => child.once('close', () => resolve()))
    let errorText = ''
    child.stderr.setEncoding('utf8').on('data', (text: string) => {
        errorText = (errorText + text).slice(-8000)
    })
    const waiterOfRequestId: { [requestId: number]: PromiseWithResolvers<unknown> } = {}
    // The workspace goes with the worker, so every request still waiting fails with it.
    const failSandbox = (error: Error) => {
        if (!controller.signal.aborted) controller.abort(error)
        for (const [requestId, waiter] of Object.entries(waiterOfRequestId)) {
            waiter.reject(error)
            delete waiterOfRequestId[Number(requestId)]
        }
    }
    child.on('error', (error) =>
        failSandbox(new Error(`The sandbox could not start. ${error.message}`)),
    )
    child.once('close', () =>
        failSandbox(new Error(`The sandbox stopped, and its files are gone. ${errorText}`)),
    )
    child.stdin.on('error', () => {}) // An exited worker is reported when it closes.
    createInterface({ input: child.stdout }).on('line', (line) => {
        let reply: z.infer<typeof replySchema>
        try {
            reply = replySchema.parse(JSON.parse(line))
        } catch {
            void close()
            return
        }
        const waiter = waiterOfRequestId[reply.id]
        delete waiterOfRequestId[reply.id]
        if ('error' in reply) waiter?.reject(new Error(reply.error))
        else waiter?.resolve(reply.result)
    })
    let nextRequestId = 0
    const send = (body: { operation: string; [key: string]: string | number }) => {
        const requestId = nextRequestId++
        child.stdin.write(JSON.stringify({ id: requestId, ...body }) + '\n')
        return requestId
    }
    let closing: Promise<void> | undefined
    const close = () => {
        if (!closing) {
            failSandbox(new Error('The sandbox was closed.'))
            // Docker removal also stops its attached CLI; closing stdin first races --rm.
            if (!container) {
                child.stdin.end()
                child.kill('SIGKILL')
            }
            closing = container
                ? promisify(execFile)('docker', ['rm', '--force', container], { timeout: 10_000 })
                      .then(() => {})
                      .catch((error: Error) => {
                          if (!error.message.includes('No such container'))
                              console.error(
                                  'Could not remove the sandbox dev container:',
                                  error.message,
                              )
                      })
                      .then(() => exited)
                : exited
        }
        return closing
    }
    // A run whose call is stopped is also stopped in the sandbox, so it doesn't keep running there. Its late reply has
    // no waiter.
    const request = async (
        body: { operation: string; [key: string]: string | number },
        signal: AbortSignal,
    ) => {
        signal.throwIfAborted()
        controller.signal.throwIfAborted()
        const waiter = Promise.withResolvers<unknown>()
        const requestId = send(body)
        waiterOfRequestId[requestId] = waiter
        const stopRequest = () => {
            delete waiterOfRequestId[requestId]
            if (body.operation === 'run') send({ operation: 'stop_run', run_id: requestId })
            waiter.reject(signal.reason)
        }
        signal.addEventListener('abort', stopRequest, { once: true })
        try {
            return await waiter.promise
        } finally {
            signal.removeEventListener('abort', stopRequest)
        }
    }
    return { request, close, signal: controller.signal }
}

// -------------------------- The editor's sandbox --------------------------
// The AI chat and its subagents share one sandbox for as long as the editor runs, so their files stay between
// replies. A sandbox that stopped is replaced the next time one is needed, without its files.
let sandbox: ReturnType<typeof openLinuxSandbox> | undefined
export const getLinuxSandbox = () => {
    if (!sandbox || sandbox.signal.aborted) sandbox = openLinuxSandbox()
    return sandbox
}
export const closeLinuxSandbox = async () => {
    await sandbox?.close()
}
