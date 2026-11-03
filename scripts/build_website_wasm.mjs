import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

// Keep npm's dev and build hooks working with the native script on each platform.
const scriptDirectoryPath = path.dirname(fileURLToPath(import.meta.url))
const isWindows = process.platform === 'win32'
const command = isWindows ? 'powershell.exe' : 'bash'
const commandArgs = isWindows
    ? ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(scriptDirectoryPath, 'windows-versions/build_website_wasm.ps1')]
    : [path.join(scriptDirectoryPath, 'build_website_wasm.sh')]
const result = spawnSync(command, commandArgs, { stdio: 'inherit' })
if (result.error) {
    throw result.error
}
if (result.signal) {
    throw new Error(`Website WebAssembly build stopped by ${result.signal}`)
}
process.exit(result.status)
