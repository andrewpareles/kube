import { test } from 'node:test'
import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import { z } from 'zod'
import { openLinuxSandbox } from '../src/helpers/linuxSandbox.ts'

// Run explicitly with npm run test:sandbox after building the dev image on macOS.
// Reuse the worker's security checks; its Rust tests cover Blender and file handling.
test('dev Docker enforces isolation, stops the command of a stopped call, and cleans up on close', async () => {
    assert.equal(process.platform, 'darwin')
    const exec = promisify(execFile)
    const sandbox = openLinuxSandbox()
    const run = async (command: string) => {
        const result = z
            .object({ exit_code: z.number(), output: z.string() })
            .parse(
                await sandbox.request(
                    { operation: 'run', command, timeout_seconds: 120 },
                    new AbortController().signal,
                ),
            )
        assert.equal(result.exit_code, 0, result.output)
        return result.output.trim()
    }
    let container: string
    try {
        container = await run('cat /etc/hostname')
        const config = await exec('docker', [
            'inspect',
            '--format',
            '{{.HostConfig.ReadonlyRootfs}} {{.HostConfig.NetworkMode}} {{len .Mounts}}',
            container,
        ])
        assert.equal(config.stdout.trim(), 'true none 0')
        const supervisor = await run('printf "%s" "$PPID"')
        const checks = await readFile(
            new URL('../../linux-sandbox/tests/sandbox.py', import.meta.url),
            'utf8',
        )
        await run(
            `python3 - <<'PY'\nsupervisor_pid=${Number(supervisor)}\nsecret_path=f'/proc/{supervisor_pid}/environ'\n${checks}\nPY`,
        )
        // Stopping a call stops its command in the sandbox, which keeps running for the next call.
        const controller = new AbortController()
        const sleeping = sandbox.request(
            {
                operation: 'run',
                command: 'sleep 300 & echo $! > sleep.pid; wait',
                timeout_seconds: 600,
            },
            controller.signal,
        )
        const pid = await run('while [ ! -s sleep.pid ]; do sleep 0.05; done; cat sleep.pid')
        controller.abort()
        await assert.rejects(sleeping, { name: 'AbortError' })
        await run(`while kill -0 ${Number(pid)} 2>/dev/null; do sleep 0.05; done`)
    } finally {
        await sandbox.close()
    }
    await assert.rejects(exec('docker', ['inspect', container]), /No such (object|container)/)
})
