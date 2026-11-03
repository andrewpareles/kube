import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'
import { readScriptDiagnostics, scriptTypeEnvironment } from '../crates/editor_server/node/src/helpers/scripts.ts'
import { getScriptSnapshots } from '../packages/website/app/editor/editorScripts.ts'

// Opt in with EDITOR_SCRIPT_BROWSER_TESTS=1 and Playwright available through NODE_PATH.
// Real React, Monaco, workers, and the server's TypeScript compiler run without a listener or network mocks.
test('script tabs retain text, undo, and cursor state while the world stays mounted', { skip: process.env.EDITOR_SCRIPT_BROWSER_TESTS !== '1', timeout: 120000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const { build } = require('esbuild')
    const postcss = require('postcss')
    const tailwind = require('@tailwindcss/postcss')
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-scripts-browser-'))
    const saveDirectory = path.join(directory, 'saved')
    const website = repoPath('packages/website')
    const authored = JSON.parse(await readFile(repoPath('game-35-latest.kube'), 'utf8'))
    const scriptIds = Object.keys(authored.scriptRawCodeOfId).map(Number)
    assert.ok(scriptIds.length >= 2)
    const [firstId, secondId] = scriptIds
    const firstName = authored.objectOfId[firstId].name
    const secondName = authored.objectOfId[secondId].name
    const snapshots = getScriptSnapshots({ world: authored, scriptDraftOfId: {} })
    const firstPath = snapshots.find(script => script.objectId === firstId)!.path
    const secondPath = snapshots.find(script => script.objectId === secondId)!.path
    const relativeImport = path.posix.relative(path.posix.dirname(firstPath), secondPath)
    const browserErrors: string[] = []
    let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined
    try {
        await build({
            stdin: { contents: `
                import React, { useEffect, useLayoutEffect, useReducer, useRef } from 'react'
                import { createRoot } from 'react-dom/client'
                import { Toaster, toast } from 'sonner'
                import * as monaco from 'monaco-editor/editor/editor.api.js'
                import { getTypeScriptWorker } from 'monaco-editor/languages/features/typescript/register.js'
                import { EditorDocumentTabs } from '@/app/editor/[game_id]/EditorDocumentTabs'
                import { EditorScriptEditor } from '@/app/editor/[game_id]/EditorScriptEditor'
                import { EditorObjectPanel } from '@/app/editor/[game_id]/EditorObjectPanel'
                import { useEditorScripts } from '@/app/editor/[game_id]/useEditorScripts'
                import { createEditorWorldState, editorWorldReducer } from '@/app/editor/editorWorld'
                import { createEditorClientActionSender } from '@/app/editor/editorSync'
                import { storedWorldSchema } from '@/app/editor/editorObjectSchema'
                import authored from '${repoPath('game-35-latest.kube')}'
                import { withIsPushable } from '${repoPath('tests/authoredWorld.ts')}'
                const world = storedWorldSchema.parse(withIsPushable(authored))
                const services = { readTypes: () => window.readScriptTypes() }
                // Stands in for the editor server: it saves script text, answers with the number of the world batch that
                // includes it, and streams that world back. It starts out held, and nothing it is sent is answered until released.
                let serverWorld = world
                let serverSeq = 0
                let releaseServer = () => {}
                let serverGate = new Promise(resolve => { releaseServer = resolve })
                const holdServer = () => { serverGate = new Promise(resolve => { releaseServer = resolve }) }
                const receiveWorld = edit => {
                    serverWorld = structuredClone(serverWorld)
                    edit(serverWorld)
                    serverSeq += 1
                    window.editorTest.dispatch({ type: 'refresh', world: serverWorld, seq: serverSeq })
                    return serverSeq
                }
                const clientActionSender = createEditorClientActionSender(async action => {
                    await serverGate
                    const savedTextOfId = {}
                    for (const delta of action.deltas) if (delta.section === 'scriptRawCodeOfId') savedTextOfId[delta.objectId] = await window.saveScript({ objectId: delta.objectId, text: delta.value })
                    return receiveWorld(world => Object.assign(world.scriptRawCodeOfId, savedTextOfId))
                })
                const App = () => {
                    const [state, dispatch] = useReducer(editorWorldReducer, world, createEditorWorldState)
                    const scripts = useEditorScripts(state, services)
                    const focusScriptRef = useRef(null)
                    // Mirrors BrowserEditor: every queued action is sent once, in order, and the server's answer acks or rejects it.
                    const sentActionsRef = useRef(new WeakSet())
                    useEffect(() => {
                        for (const action of state.actionsToSend) {
                            if (sentActionsRef.current.has(action)) continue
                            sentActionsRef.current.add(action)
                            void clientActionSender.send(action).then(seq => dispatch({ type: 'acked', action, seq }), error => {
                                dispatch({ type: 'rejected', action })
                                toast.error('Could not save script', { description: String(error), id: 'editor-script-save-error' })
                            })
                        }
                    }, [state.actionsToSend])
                    useLayoutEffect(() => { window.editorTest = { state, dispatch, monaco, originalWorld: world, scripts, getTypeScriptWorker, holdServer, releaseServer: () => releaseServer(), receiveWorld } })
                    return <main style={{ display: 'flex', height: '100vh' }}>
                        <Toaster />
                        <EditorDocumentTabs state={state} dispatch={dispatch} focusScriptRef={focusScriptRef} scriptEditor={<EditorScriptEditor gameId={35} state={state} dispatch={dispatch} scripts={scripts} focusRef={focusScriptRef} />}>
                            <canvas aria-label="World view" />
                        </EditorDocumentTabs>
                        <div style={{ width: 320, flexShrink: 0 }}><EditorObjectPanel editor={{ state, dispatch, onSelect: objectId => dispatch({ type: 'select', objectId }), onFrame: () => {} }} /></div>
                    </main>
                }
                createRoot(document.getElementById('root')).render(<App />)
            `, resolveDir: website, loader: 'tsx' },
            outfile: path.join(directory, 'app.js'), bundle: true, format: 'esm', jsx: 'automatic',
            tsconfig: path.join(website, 'tsconfig.json'), loader: { '.ttf': 'file', '.kube': 'json' },
            define: { 'process.env.NODE_ENV': '"production"' },
        })
        // esbuild leaves new URL worker imports intact; emit the actual Monaco worker at that relative path.
        await build({ entryPoints: [require.resolve('monaco-editor/editor/editor.worker.js')], outfile: path.join(directory, 'monaco-editor/editor/editor.worker.js'), bundle: true, format: 'esm' })
        await build({ entryPoints: [require.resolve('monaco-editor/languages/features/typescript/ts.worker.js')], outfile: path.join(directory, 'monaco-editor/languages/features/typescript/ts.worker.js'), bundle: true, format: 'esm' })
        const css = await postcss([tailwind({ base: website })]).process('@import "tailwindcss"; @import "tw-animate-css";', { from: path.join(website, 'editor-test.css') })
        await writeFile(path.join(directory, 'styles.css'), css.css)
        await writeFile(path.join(directory, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="styles.css"><link rel="stylesheet" href="app.css"></head><body><div id="root"></div><script type="module" src="app.js"></script></body></html>')
        browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
        const page = await browser.newPage({ viewport: { width: 1200, height: 800 } })
        let typeRequests = 0
        let saveRequests = 0
        await page.exposeFunction('readScriptTypes', () => { typeRequests++; return scriptTypeEnvironment })
        // Persist shortcut callbacks to real temporary files; this test never connects to the game's database.
        await page.exposeFunction('saveScript', async (script: { objectId: number, text: string }) => {
            saveRequests++
            await writeFile(path.join(saveDirectory, `${script.objectId}.tsx`), script.text)
            return readFile(path.join(saveDirectory, `${script.objectId}.tsx`), 'utf8')
        })
        page.on('pageerror', (error: Error) => browserErrors.push(error.message))
        page.on('console', (message: { type: () => string, text: () => string }) => { if (message.type() === 'error') browserErrors.push(message.text()) })
        await page.goto(pathToFileURL(path.join(directory, 'index.html')).href)
        const worldCanvas = await page.getByLabel('World view').elementHandle()
        assert.ok(worldCanvas)
        assert.equal(await page.getByRole('tablist', { name: 'Editor documents' }).count(), 0)
        // Reopening a branch must leave the keyboard's row in view, even with a selection far below it.
        const tree = page.getByRole('tree', { name: 'Objects' })
        await tree.getByRole('treeitem').last().click()
        const selectedId = await page.evaluate('(window.editorTest.state.selectedObjectIds.at(-1) ?? null)')
        await page.keyboard.press('Home')
        const rootRow = page.locator(`[data-object-id="${authored.rootObjectId}"]`)
        await page.keyboard.press('ArrowLeft')
        assert.equal(await rootRow.getAttribute('aria-expanded'), 'false')
        const scrollTop = await tree.evaluate((element: HTMLElement) => element.scrollTop)
        await page.keyboard.press('ArrowRight')
        assert.equal(await rootRow.getAttribute('aria-expanded'), 'true')
        assert.equal(await tree.evaluate((element: HTMLElement) => element.scrollTop), scrollTop, 'Expanding must not scroll back to the old selection')
        assert.equal(await rootRow.evaluate((element: HTMLElement) => element === document.activeElement), true)
        assert.equal(await page.evaluate('(window.editorTest.state.selectedObjectIds.at(-1) ?? null)'), selectedId)
        const selectScript = async (id: number) => {
            await page.getByRole('textbox', { name: 'Search by name or id' }).fill(String(id))
            await page.locator(`[data-object-id="${id}"]`).click()
        }
        await selectScript(firstId)
        await page.waitForFunction(`window.editorTest.monaco.editor.getEditors().length === 1`)
        await page.waitForFunction('window.editorTest.scripts.typeEnvironment !== null')
        // Completions must come from the actual game API, with useful hover types and a visible suggest menu.
        await page.evaluate(`window.editorTest.monaco.editor.getEditors()[0].setValue('game.objectService.')`)
        const completions = await page.evaluate(`(async () => {
            const { monaco, getTypeScriptWorker } = window.editorTest
            const model = monaco.editor.getEditors()[0].getModel()
            const worker = await (await getTypeScriptWorker())(model.uri)
            const completions = await worker.getCompletionsAtPosition(model.uri.toString(), model.getValueLength())
            const info = await worker.getQuickInfoAtPosition(model.uri.toString(), 7)
            return { names: completions.entries.map(entry => entry.name), hover: info.displayParts.map(part => part.text).join('') }
        })()`)
        assert.ok(completions.names.includes('getObjectById'))
        assert.ok(completions.names.includes('getObjectsByType'))
        assert.match(completions.hover, /ObjectService/)
        await page.evaluate(`(() => { const editor = window.editorTest.monaco.editor.getEditors()[0]; editor.setValue('game.objectService.getObjectBy'); editor.setPosition({ lineNumber: 1, column: editor.getValue().length + 1 }); editor.trigger('test', 'editor.action.triggerSuggest', {}) })()`)
        await page.locator('.suggest-widget').getByText('getObjectById', { exact: true }).waitFor()
        await page.keyboard.press('Escape')
        const requestsAfterSetup = typeRequests
        await page.context().setOffline(true)

        // A draft in an unopened script must participate in imports, without asking the server to check text.
        await page.evaluate(`window.editorTest.dispatch({ type: 'edit-script', objectId: ${secondId}, text: 'export type Value = { count: number };' })`)
        const importText = `import type { Value } from '${relativeImport}'; const value: Value = { count: "one" };`
        await page.evaluate((text: string) => window.editorTest.monaco.editor.getEditors()[0].setValue(text), importText)
        await page.waitForFunction(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).some(marker => marker.message.includes('not assignable'))`)
        const browserDiagnostics = await page.evaluate(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).map(marker => marker.message)`)
        const serverDiagnostics = readScriptDiagnostics([{ objectId: firstId, path: firstPath, text: importText }, { objectId: secondId, path: secondPath, text: 'export type Value = { count: number };' }])
        assert.deepEqual(browserDiagnostics, serverDiagnostics[firstId].map(diagnostic => diagnostic.message))
        assert.equal(await page.getByRole('tab', { name: secondName, exact: true }).count(), 0)
        assert.equal(await page.evaluate((scriptPath: string) => window.editorTest.monaco.editor.getModels().some(model => model.uri.path.endsWith(`/${scriptPath}.tsx`)), secondPath), true, 'Related diagnostics create a background model for the imported type')
        await page.evaluate(`window.editorTest.dispatch({ type: 'edit-script', objectId: ${secondId}, text: 'export type Value = { count: string };' })`)
        await page.waitForFunction(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).length === 0`)
        assert.equal(await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getValue()'), importText, 'Changing a background import must refresh diagnostics without changing the active text')
        await page.evaluate(`window.editorTest.monaco.editor.getEditors()[0].setValue('const element = <div className={42}>Hello</div>;')`)
        await page.waitForFunction(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).some(marker => marker.message.includes('not assignable'))`)
        await page.evaluate(`window.editorTest.monaco.editor.getEditors()[0].setValue('const element = <div className="p-2">Hello</div>;')`)
        await page.waitForFunction(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).length === 0`)
        assert.equal(typeRequests, requestsAfterSetup, 'Typing and checking must stay local after the declarations arrive')
        await page.context().setOffline(false)
        await page.locator('.monaco-editor').click()
        await page.keyboard.press('Meta+a')
        await page.keyboard.insertText('const wrong: number = "bad";')
        await page.waitForFunction(`window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).some(marker => marker.message.includes('not assignable'))`)
        assert.ok(await page.locator('.monaco-editor .squiggly-error').count() > 0)
        await page.keyboard.press('Meta+a')
        await page.keyboard.insertText('const firstDraft = 1;')
        await page.waitForFunction(`window.editorTest.state.scriptDraftOfId[${firstId}] === 'const firstDraft = 1;'`)
        await page.waitForFunction(`window.editorTest.scripts.typeEnvironment && window.editorTest.monaco.editor.getModelMarkers({ resource: window.editorTest.monaco.editor.getEditors()[0].getModel().uri }).length === 0`)
        assert.equal(await page.evaluate('window.editorTest.state.world === window.editorTest.originalWorld'), true)
        await page.keyboard.press('ArrowLeft')
        const firstPosition = await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getPosition()')
        await selectScript(secondId)
        await page.locator('.monaco-editor').click()
        await page.keyboard.press('Meta+a')
        await page.keyboard.insertText('const secondDraft = 2;')
        await page.getByRole('tab', { name: firstName }).click()
        assert.equal(await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getValue()'), 'const firstDraft = 1;')
        assert.deepEqual(await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getPosition()'), firstPosition)
        await page.locator('.monaco-editor').click()
        await page.keyboard.press('Meta+z')
        await page.waitForFunction(`window.editorTest.monaco.editor.getEditors()[0].getValue() !== 'const firstDraft = 1;'`)
        assert.equal(await page.evaluate(`(window.editorTest.state.scriptDraftOfId[${firstId}] ?? window.editorTest.state.world.scriptRawCodeOfId[${firstId}]) === window.editorTest.monaco.editor.getEditors()[0].getValue()`), true)
        await page.keyboard.press('Meta+Shift+z')
        await page.waitForFunction(`window.editorTest.state.scriptDraftOfId[${firstId}] === 'const firstDraft = 1;'`)
        await page.getByRole('tab', { name: 'World', exact: true }).click()
        assert.equal(await worldCanvas.isVisible(), true, 'The original world element stays mounted')
        await page.getByRole('tab', { name: secondName }).click()
        assert.equal(await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getValue()'), 'const secondDraft = 2;')
        // Test the browser-safe chord; injected Meta+W does not exercise Chrome's reserved native shortcut.
        await page.locator('.monaco-editor').click()
        await page.keyboard.press('Meta+k')
        await page.keyboard.press('w')
        await page.waitForFunction(`window.editorTest.state.activeScriptId === ${firstId} && !window.editorTest.state.openScriptIds.includes(${secondId})`)
        assert.equal(page.isClosed(), false)
        assert.equal(await page.evaluate(`window.editorTest.state.scriptDraftOfId[${secondId}]`), 'const secondDraft = 2;')
        await page.keyboard.press('Meta+k')
        await page.keyboard.press('w')
        await page.waitForFunction('window.editorTest.state.openScriptIds.length === 0')
        assert.equal(await worldCanvas.isVisible(), true)
        assert.equal(await page.getByRole('tablist', { name: 'Editor documents' }).count(), 0)
        assert.deepEqual(await page.evaluate('window.editorTest.monaco.editor.getModels().map(model => model.uri.toString())'), [])
        await selectScript(firstId)
        await page.waitForFunction(`window.editorTest.monaco.editor.getEditors()[0]?.getValue() === 'const firstDraft = 1;'`)
        // Typing itself reaches the server. While the directory is missing it refuses every write, and each draft
        // goes back to the server's text.
        await page.evaluate('window.editorTest.releaseServer()')
        const saveError = page.locator('[data-sonner-toast][data-type="error"]')
        await saveError.getByText('Could not save script', { exact: true }).waitFor()
        assert.equal(await page.locator('main > :not([aria-label^="Notifications"]) [role="alert"]').count(), 0)
        await page.waitForFunction('window.editorTest.state.actionsToSend.length === 0 && Object.keys(window.editorTest.state.scriptDraftOfId).length === 0')
        assert.equal(await page.evaluate(`window.editorTest.state.world.scriptRawCodeOfId[${firstId}]`), authored.scriptRawCodeOfId[firstId])
        await page.waitForFunction((text: string) => window.editorTest.monaco.editor.getEditors()[0].getValue() === text, authored.scriptRawCodeOfId[firstId])

        // Once writes succeed, what is typed streams through and the draft clears on the server's answer.
        await mkdir(saveDirectory)
        await page.locator('.monaco-editor').click()
        await page.keyboard.press('Meta+a')
        await page.keyboard.insertText('const saved = 5;')
        await page.waitForFunction(`window.editorTest.state.world.scriptRawCodeOfId[${firstId}] === 'const saved = 5;' && window.editorTest.state.scriptDraftOfId[${firstId}] === undefined`)
        assert.equal(await readFile(path.join(saveDirectory, `${firstId}.tsx`), 'utf8'), 'const saved = 5;')
        assert.ok(saveRequests > 0)
        // Remote updates must reach untouched text without discarding a draft the server has not answered yet, or
        // leaving deleted tabs behind.
        await page.evaluate('window.editorTest.holdServer()')
        await page.keyboard.press('Meta+a')
        await page.keyboard.insertText('const localAfterSave = 4;')
        await page.evaluate(`window.editorTest.receiveWorld(world => {
            world.scriptRawCodeOfId[${firstId}] = 'const remote = 3;'
            delete world.objectOfId[${secondId}]
        })`)
        assert.equal(await page.evaluate('window.editorTest.monaco.editor.getEditors()[0].getValue()'), 'const localAfterSave = 4;')
        await page.getByRole('button', { name: `Close ${firstName}`, exact: true }).click()
        assert.deepEqual(await page.evaluate('window.editorTest.monaco.editor.getModels().map(model => model.uri.toString())'), [])
        assert.deepEqual(browserErrors, [])
    } finally {
        await browser?.close()
        await rm(directory, { recursive: true, force: true })
    }
})
