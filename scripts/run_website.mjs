import { spawn, spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { createRequire } from 'node:module'
import { delimiter } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = new URL('../', import.meta.url)
const website = new URL('packages/website/', root)
const require = createRequire(new URL('package.json', website))
const { loadEnvConfig } = require('@next/env')
loadEnvConfig(fileURLToPath(website), true)

// Reuse the website's existing environment files and credentials for all dev services.
Object.assign(process.env, {
    NODE_ENV: 'development',
    EDITOR_SERVER_SECRET: randomBytes(32).toString('hex'),
    CHROMIUM_PATH: process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
        : process.platform === 'win32' ? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe' : 'chromium',
    PATH: `${fileURLToPath(new URL('target/debug/', root))}${delimiter}${process.env.PATH}`,
})
const required = ['DRIZZLE_DATABASE_URL', 'SUPABASE_URL', 'SUPABASE_SECRET_KEY']
const missing = required.filter(key => !process.env[key]?.trim())
if (missing.length) {
    console.error(`Set these in packages/website/.env.local or export them before running: ${missing.join(', ')}`)
    process.exit(1)
}

// Build before starting the website so failed builds leave no background processes.
for (const [command, args] of [
    process.platform === 'win32'
        ? ['cmd.exe', ['/d', '/s', '/c', 'npm --prefix packages/kube-ts-api run bundle']]
        : ['npm', ['--prefix', 'packages/kube-ts-api', 'run', 'bundle']],
    process.platform === 'win32'
        ? ['cmd.exe', ['/d', '/s', '/c', 'npm --prefix packages/play-webview run bundle']]
        : ['npm', ['--prefix', 'packages/play-webview', 'run', 'bundle']],
    ['cargo', ['build', '--locked', '-p', 'editor_server', '-p', 'linux_sandbox', '-p', 'client']],
    [process.execPath, ['scripts/build_website_wasm.mjs']],
]) {
    const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' })
    if (result.error) throw result.error
    if (result.status !== 0) process.exit(result.status ?? 1)
}

const child = spawn(process.execPath, ['node_modules/next/dist/bin/next', 'dev', '--port', '3000'], { cwd: website, stdio: 'inherit', env: { ...process.env, PORT: '3000' } })
let isStopping = false
const stop = () => {
    if (isStopping) return
    isStopping = true
    child.kill('SIGTERM')
}
child.once('error', error => { console.error(error); process.exit(1) })
child.once('close', code => process.exit(isStopping ? 0 : (code ?? 1)))
process.once('SIGINT', stop)
process.once('SIGTERM', stop)
console.log('Website: http://localhost:3000 — editors and playtests run in dev as games are opened.')
