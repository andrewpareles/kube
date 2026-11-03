import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
    closeLinuxSandbox,
    getLinuxSandbox,
    openLinuxSandbox,
} from '../src/helpers/linuxSandbox.ts'

// With an empty PATH there is no worker or Docker executable, so every sandbox fails to start.
const runWithoutExecutables = async (run: () => Promise<void>) => {
    const path = process.env.PATH
    process.env.PATH = ''
    try {
        await run()
    } finally {
        process.env.PATH = path
    }
}

test('a missing worker or Docker executable rejects instead of hanging or crashing Node', () =>
    runWithoutExecutables(async () => {
        const sandbox = openLinuxSandbox()
        try {
            await assert.rejects(
                sandbox.request(
                    { operation: 'run', command: 'echo test', timeout_seconds: 10 },
                    new AbortController().signal,
                ),
                /ENOENT/,
            )
        } finally {
            await sandbox.close()
        }
    }))

test('a call that was already stopped never reaches the sandbox', () =>
    runWithoutExecutables(async () => {
        const sandbox = openLinuxSandbox()
        try {
            await assert.rejects(
                sandbox.request(
                    { operation: 'run', command: 'echo test', timeout_seconds: 10 },
                    AbortSignal.abort(),
                ),
                { name: 'AbortError' },
            )
        } finally {
            await sandbox.close()
        }
    }))

test('the editor replaces a sandbox that stopped the next time one is needed', () =>
    runWithoutExecutables(async () => {
        const stopped = getLinuxSandbox()
        try {
            await assert.rejects(
                stopped.request(
                    { operation: 'read_file', path: 'model.glb' },
                    new AbortController().signal,
                ),
                /ENOENT/,
            )
            const replacement = getLinuxSandbox()
            assert.notEqual(replacement, stopped)
        } finally {
            await closeLinuxSandbox()
        }
    }))
