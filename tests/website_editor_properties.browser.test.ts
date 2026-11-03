import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'

// Real Chrome is needed: DOM emulators do not reproduce native stepping with large float32 bounds.
test('property steppers change tile sizes while integer fields remain plain inputs', { skip: process.env.EDITOR_PROPERTY_BROWSER_TESTS !== '1', timeout: 60000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const { build } = require('esbuild')
    const postcss = require('postcss')
    const tailwind = require('@tailwindcss/postcss')
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-properties-browser-'))
    const website = repoPath('packages/website')
    const authored = JSON.parse(await readFile(repoPath('game-35-latest.kube'), 'utf8'))
    const brickId = Object.keys(authored.objectOfId).find(id => authored.objectOfId[id].info.Brick)
    const materialId = Object.keys(authored.objectOfId).find(id => authored.objectOfId[id].info.Material)
    const animatorId = Object.keys(authored.objectOfId).find(id => authored.objectOfId[id].info.PropertyAnimator)
    const lightingId = Object.keys(authored.objectOfId).find(id => authored.objectOfId[id].info.WorldLighting)
    assert.ok(brickId && materialId && animatorId && lightingId)
    let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined
    try {
        await build({
            stdin: { contents: `
                import React, { useReducer, useLayoutEffect } from 'react'
                import { createRoot } from 'react-dom/client'
                import { EditorObjectPanel } from '@/app/editor/[game_id]/EditorObjectPanel'
                import { createEditorWorldState, editorWorldReducer } from '@/app/editor/editorWorld'
                import { storedWorldSchema } from '@/app/editor/editorObjectSchema'
                import authored from '${repoPath('game-35-latest.kube')}'
                import { withIsPushable } from '${repoPath('tests/authoredWorld.ts')}'
                const world = storedWorldSchema.parse(withIsPushable(authored))
                // The authored fixture predates this material property, which the editor server now supplies.
                for (const object of Object.values(world.objectOfId)) if (object.info.Material) object.info.Material.isDoubleSided = false
                // It also predates glow on every object's visual settings, which the editor server fills in today.
                for (const object of Object.values(world.objectOfId)) for (const info of Object.values(object.info)) if (info?.visualSettings) info.visualSettings.glow = 0
                // And the shadow softness, ambient occlusion, sky reflection and vibrance settings on WorldLighting.
                for (const object of Object.values(world.objectOfId)) if (object.info.WorldLighting) object.info.WorldLighting = { ...object.info.WorldLighting, sunShadowSoftness: 2, ambientOcclusionIntensity: 0.7, ambientOcclusionRadius: 1, ambientOcclusionSunlightStrength: 0.5, skyReflectionStrength: 1, vibrance: 0.2 }
                const App = () => {
                    const [state, dispatch] = useReducer(editorWorldReducer, world, createEditorWorldState)
                    useLayoutEffect(() => { window.editorTest = { state, dispatch } })
                    return <EditorObjectPanel editor={{ state, dispatch, onSelect: (objectId, isMultiSelect) => dispatch({ type: isMultiSelect ? 'toggle-select' : 'select', objectId }), onFrame: () => {} }} />
                }
                createRoot(document.getElementById('root')).render(<App />)
            `, resolveDir: website, loader: 'tsx' },
            outfile: path.join(directory, 'app.js'), bundle: true, format: 'esm', jsx: 'automatic',
            tsconfig: path.join(website, 'tsconfig.json'), loader: { '.kube': 'json' },
            define: { 'process.env.NODE_ENV': '"production"' },
        })
        const css = await postcss([tailwind({ base: website })]).process('@import "tailwindcss"; @import "tw-animate-css";', { from: path.join(website, 'editor-test.css') })
        await writeFile(path.join(directory, 'styles.css'), css.css)
        await writeFile(path.join(directory, 'index.html'), '<!doctype html><html><head><link rel="stylesheet" href="styles.css"></head><body><div id="root" style="height:100vh;width:384px"></div><script type="module" src="app.js"></script></body></html>')
        browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
        const page = await browser.newPage()
        const errors: string[] = []
        page.on('pageerror', (error: Error) => errors.push(error.message))
        await page.goto(pathToFileURL(path.join(directory, 'index.html')).href)
        await page.waitForFunction('window.editorTest !== undefined')
        await page.evaluate(`window.editorTest.dispatch({ type: 'select', objectId: ${brickId} })`)
        const enableMaterial = page.getByRole('checkbox', { name: 'Enable Material', exact: true })
        if (await enableMaterial.getAttribute('aria-checked') !== 'true') await enableMaterial.click()
        for (const axis of ['U', 'V']) {
            const field = page.getByRole('spinbutton', { name: `Tile Size ${axis}`, exact: true })
            await field.fill('1')
            await field.press('Enter')
            await field.press('ArrowUp')
            assert.equal(await field.inputValue(), '1.05')
            await field.press('Enter')
            assert.equal(await page.evaluate(`window.editorTest.state.world.objectOfId[${brickId}].info.Brick.material.tileSize${axis}`), 1.05)
            await field.press('ArrowDown')
            assert.equal(await field.inputValue(), '1')
            await field.press('Enter')
            // Exercise the native spinner with the pointer as well as the keyboard.
            await field.hover()
            const bounds = await field.boundingBox()
            assert.ok(bounds)
            await page.mouse.click(bounds.x + bounds.width - 15, bounds.y + bounds.height * 0.35)
            assert.equal(await field.inputValue(), '1.05')
            await field.press('Enter')
        }
        for (const [id, label] of [[materialId, 'Albedo Texture Data Id'], [animatorId, 'Priority']]) {
            await page.evaluate(`window.editorTest.dispatch({ type: 'select', objectId: ${id} })`)
            if (id === materialId) {
                const enable = page.getByRole('checkbox', { name: `Enable ${label}`, exact: true })
                if (await enable.getAttribute('aria-checked') !== 'true') await enable.click()
            }
            const field = page.getByRole('textbox', { name: label, exact: true })
            await field.fill('2.5')
            await field.press('Enter')
            assert.equal(await field.inputValue(), '3', 'Integer entry still rounds on commit')
            await field.press('ArrowUp')
            await field.press('ArrowDown')
            assert.equal(await field.inputValue(), '3', 'Arrow keys must not change an integer field')
        }
        await page.evaluate(`window.editorTest.dispatch({ type: 'select', objectId: ${lightingId} })`)
        const phi = page.getByRole('spinbutton', { name: 'Phi', exact: true })
        await phi.click()
        await phi.press('Meta+a')
        await phi.pressSequentially('30')
        assert.equal(await phi.inputValue(), '30')
        await phi.press('Enter')
        assert.equal(await phi.inputValue(), '30')
        await phi.press('ArrowUp')
        assert.ok(Math.abs(Number(await phi.inputValue()) - 30.05) < 1e-10, 'Degree conversion must not prevent native steps')
        // A name is kept without spaces and never empty, but the field shows what was typed until focus leaves it.
        await page.evaluate(`window.editorTest.dispatch({ type: 'select', objectId: ${brickId} })`)
        const name = page.getByRole('textbox', { name: 'Name', exact: true })
        await name.and(page.locator(`[value=${JSON.stringify(authored.objectOfId[brickId].name)}]`)).waitFor()
        await name.fill('')
        assert.equal(await name.inputValue(), '')
        assert.equal(await page.evaluate(`window.editorTest.state.world.objectOfId[${brickId}].name`), '-')
        await name.pressSequentially('my door')
        assert.equal(await name.inputValue(), 'my door')
        assert.equal(await page.evaluate(`window.editorTest.state.world.objectOfId[${brickId}].name`), 'my-door')
        await name.press('Enter')
        assert.equal(await name.inputValue(), 'my-door')
        assert.deepEqual(errors, [])
    } finally {
        await browser?.close()
        await rm(directory, { recursive: true, force: true })
    }
})
