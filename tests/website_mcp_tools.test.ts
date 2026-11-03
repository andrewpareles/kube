import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import test from 'node:test'
import { MCP_ONLY_TOOLS } from '../packages/website/app/api/ai-mcp/[gameId]/mcpTools.ts'

const runMcpOnlyTool = async (name: string, args: unknown) => {
    const mcpOnlyTool = MCP_ONLY_TOOLS.find(({ tool }) => tool.name === name)
    assert.ok(mcpOnlyTool)
    return mcpOnlyTool.run(args, { inviteToken: 'invite-code', readSystemPrompt: async () => 'The prompt.', readEditorUrl: async () => 'https://editor.test/request-id' })
}

// The AI runs a file tool's command in its own shell, so a real shell splits it here, with curl swapped for a function
// that prints each argument it was given.
const readCurlArgs = (command: string) => execFileSync('sh', ['-c', `curl() { printf '%s\\0' "$@"; }; ${command}`], { encoding: 'utf8' }).split('\0').slice(0, -1)
const MCP_HEADER_ARGS = ['-H', 'Authorization: Invite invite-code', '-H', 'x-editor-session: mcp']

test('each file tool answers with a curl command that sends the file to the editor with the game invite', async () => {
    const filePath = `/tmp/it's a "tree" & more.glb`
    assert.deepEqual(readCurlArgs(await runMcpOnlyTool('asset_import_from_file', { file_path: filePath, kind: 'Mesh', name: 'Tall tree & co', should_publish: true, asset_id: null })), [
        '-sS', '--data-binary', `@${filePath}`, ...MCP_HEADER_ARGS, 'https://editor.test/request-id/asset-import?kind=Mesh&name=Tall+tree+%26+co&should_publish=true',
    ])
    assert.deepEqual(readCurlArgs(await runMcpOnlyTool('asset_download_source_to_file', { file_path: '/tmp/my tree.glb', asset_id: 7 })), [
        '-sS', '--fail', '-o', '/tmp/my tree.glb', '-w', '%{content_type}\\n', ...MCP_HEADER_ARGS, 'https://editor.test/request-id/asset-download-source?asset_id=7',
    ])
    assert.deepEqual(readCurlArgs(await runMcpOnlyTool('download_world_to_file', { file_path: '/tmp/my world.kube' })), [
        '-sS', '--fail', '-o', '/tmp/my world.kube', ...MCP_HEADER_ARGS, 'https://editor.test/request-id/download-world',
    ])
    assert.deepEqual(readCurlArgs(await runMcpOnlyTool('replace_world_with_file', { file_path: '/tmp/my world.kube' })), [
        '-sS', '--data-binary', '@/tmp/my world.kube', ...MCP_HEADER_ARGS, 'https://editor.test/request-id/upload-world',
    ])
})

test('an imported file is sent as the kind the AI gives, so an image can be a texture or a skybox', async () => {
    const readKind = async (kind: string) => new URL(readCurlArgs(await runMcpOnlyTool('asset_import_from_file', { file_path: '/tmp/sky.png', kind, name: 'Sky', should_publish: true, asset_id: null })).at(-1) ?? '').searchParams.get('kind')
    assert.equal(await readKind('Texture'), 'Texture')
    assert.equal(await readKind('Skybox'), 'Skybox')
    await assert.rejects(runMcpOnlyTool('asset_import_from_file', { file_path: '/tmp/sky.png', kind: 'Material', name: 'Sky' }), /kind/)
    await assert.rejects(runMcpOnlyTool('asset_import_from_file', { file_path: '/tmp/sky.png', name: 'Sky' }), /kind/)
})

// The editor reads a missing asset_id as none, and an empty one as a mistake, so a null asset_id is left out.
test('an upload is published only when the AI asks, and a new version names its asset', async () => {
    const readQuery = async (args: object) => new URL(readCurlArgs(await runMcpOnlyTool('asset_import_from_file', { file_path: '/tmp/tree.glb', kind: 'Mesh', name: 'Tree', ...args })).at(-1) ?? '').searchParams
    const unpublishedUpload = await readQuery({ should_publish: false, asset_id: null })
    assert.equal(unpublishedUpload.get('should_publish'), 'false')
    assert.equal(unpublishedUpload.has('asset_id'), false)
    assert.equal((await readQuery({ should_publish: true, asset_id: 12 })).get('asset_id'), '12')
    await assert.rejects(runMcpOnlyTool('asset_import_from_file', { file_path: '/tmp/tree.glb', kind: 'Mesh', name: 'Tree' }), /should_publish/)
})

// curl ends a form field's file name at a ; or , unless the name is in double quotes, so a material's images are quoted
// for curl as well as for the shell.
test('a material sends one form field for each image it was given', async () => {
    const albedoFilePath = `/tmp/bark "color"; v2, final.png`
    const command = await runMcpOnlyTool('asset_import_material_from_files', { name: 'Bark', albedo_file_path: albedoFilePath, normal_file_path: null, metallic_file_path: '/tmp/bark metal.png', should_publish: true, asset_id: null })
    assert.deepEqual(readCurlArgs(command), [
        '-sS', '-F', 'albedo=@"/tmp/bark \\"color\\"; v2, final.png"', '-F', 'metallic=@"/tmp/bark metal.png"', ...MCP_HEADER_ARGS, 'https://editor.test/request-id/asset-import-material?name=Bark&should_publish=true',
    ])
    await assert.rejects(runMcpOnlyTool('asset_import_material_from_files', { name: 'Bark' }), /albedo_file_path/)
})
