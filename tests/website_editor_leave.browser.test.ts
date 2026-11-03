import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'

// Exercise real browser dialogs and history without a server or mocked browser APIs.
test('editor leave always warns on reloads and history navigation, and stops when unmounted', { skip: process.env.EDITOR_LEAVE_BROWSER_TESTS !== '1', timeout: 60000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const { build } = require('esbuild')
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-leave-browser-'))
    let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined
    try {
        await build({
            stdin: { contents: `
                import { confirmEditorLeave, listenForEditorLeave } from '${repoPath('packages/website/app/editor/editorLeaveWarning.ts')}'
                const stop = listenForEditorLeave()
                document.querySelector('#leave').onclick = () => {
                    if (confirmEditorLeave()) history.pushState(null, '', '?left')
                }
                document.querySelector('#stop').onclick = stop
                history.replaceState(null, '', '?game')
                history.pushState(null, '', '?editor')
            `, resolveDir: repoPath('packages/website'), loader: 'ts' },
            outfile: path.join(directory, 'app.js'), bundle: true, format: 'esm',
        })
        await writeFile(path.join(directory, 'index.html'), '<!doctype html><input aria-label="Draft"><button id="leave">Leave</button><button id="stop">Stop guard</button><script type="module" src="app.js"></script>')
        browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
        const page = await browser.newPage()
        const errors: string[] = []
        page.on('pageerror', (error: Error) => errors.push(error.message))
        await page.goto(pathToFileURL(path.join(directory, 'index.html')).href)
        await page.waitForURL('**?editor')
        const draft = page.getByRole('textbox', { name: 'Draft' })

        // Nothing has been typed and leaving still warns, because the guard inspects no state at all.
        const emptyDialog = page.waitForEvent('dialog')
        const emptyLeave = page.getByRole('button', { name: 'Leave', exact: true }).click()
        await (await emptyDialog).dismiss()
        await emptyLeave
        assert.ok(page.url().endsWith('?editor'))

        await draft.fill('unsaved script')

        // Closing/reloading uses a native beforeunload dialog, preserving text when cancelled.
        const reloadDialog = page.waitForEvent('dialog')
        const reload = page.reload().catch(() => {})
        const reloadWarning = await reloadDialog
        assert.equal(reloadWarning.type(), 'beforeunload')
        await reloadWarning.dismiss()
        await reload
        assert.equal(await draft.inputValue(), 'unsaved script')

        // The editor's own link and browser Back both allow the user to stay.
        const linkDialog = page.waitForEvent('dialog')
        const leave = page.getByRole('button', { name: 'Leave', exact: true }).click()
        await (await linkDialog).dismiss()
        await leave
        assert.ok(page.url().endsWith('?editor'))
        const backDialog = page.waitForEvent('dialog')
        await page.evaluate('history.back()')
        await (await backDialog).dismiss()
        assert.ok(page.url().endsWith('?editor'))
        assert.equal(await draft.inputValue(), 'unsaved script')

        // Explicit confirmation permits navigation.
        const acceptedDialog = page.waitForEvent('dialog')
        const acceptedLeave = page.getByRole('button', { name: 'Leave', exact: true }).click()
        await (await acceptedDialog).accept()
        await acceptedLeave
        await page.waitForURL('**?left')

        // Unmount cleanup must remove both listeners, so neither Back nor reload asks again.
        await page.getByRole('button', { name: 'Stop guard' }).click()
        await page.evaluate('history.back()')
        await page.waitForURL('**?editor')
        await page.reload()
        assert.deepEqual(errors, [])
    } finally {
        await browser?.close()
        await rm(directory, { recursive: true, force: true })
    }
})
