import assert from 'node:assert/strict'
import { after, test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { repoPath } from './testPaths.ts'

// Bundle the production browser request helper with its real dependencies; no server or auth mocks.
const require = createRequire(repoPath('packages/website/package.json'))
const { build } = require('esbuild')
const directory = await mkdtemp(path.join(tmpdir(), 'editor-auth-test-'))
after(() => rm(directory, { recursive: true, force: true }))
const outfile = path.join(directory, 'editorApi.cjs')
await build({ entryPoints: [repoPath('packages/website/app/editor/editorApi.ts')], outfile, bundle: true, platform: 'node', format: 'cjs', tsconfig: repoPath('packages/website/tsconfig.json') })
const { editorHeaders, editorRequest } = require(outfile)

test('guest requests work without a login and invitations never become a shared browser credential', async () => {
    const first = await editorHeaders('first-game-token')
    const second = await editorHeaders('second-game-token')
    assert.equal(first.Authorization, 'Invite first-game-token')
    assert.equal(second.Authorization, 'Invite second-game-token')
    assert.deepEqual(await editorHeaders('first-game-token'), first)
    await assert.rejects(editorHeaders(null), 'An owner request must still require its own login')

    // Native fetch can exercise the complete request path without making any network request.
    const response = await editorRequest('first-game-token', 'data:application/json,', '%7B%22ready%22%3Atrue%7D', 'GET', undefined, undefined)
    assert.deepEqual(await response.json(), { ready: true })
})
