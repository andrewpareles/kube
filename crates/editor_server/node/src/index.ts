import { serve } from '@hono/node-server'
import { app } from './endpoints.ts'
import { editorRuntime } from './helpers/worker.ts'
import { failRepliesLeftRunning, interruptChat } from './helpers/chat.ts'
import { closeLinuxSandbox } from './helpers/linuxSandbox.ts'
import { env, isDev } from './helpers/env.ts'

await failRepliesLeftRunning()
// Rust keeps stdin open for this child's lifetime and waits for its listener to be ready.
const server = serve(
    { fetch: app.fetch, hostname: isDev ? '127.0.0.1' : '0.0.0.0', port: env.EDITOR_CHAT_PORT },
    () => process.stdout.write('ready\n'),
)
const shutdown = async () => {
    if (editorRuntime.isStopping) return
    editorRuntime.isStopping = true
    await interruptChat()
    await closeLinuxSandbox()
    server.close()
    process.exit(0)
}
const stop = () => {
    void shutdown().catch((error) => {
        console.error(error)
        process.exit(1)
    })
}
process.once('SIGINT', stop)
process.once('SIGTERM', stop)
process.stdin.once('end', stop)
process.stdin.resume()
