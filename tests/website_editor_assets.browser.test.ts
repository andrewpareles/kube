import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'

// Opt in with EDITOR_ASSET_BROWSER_TESTS=1 and Playwright available through NODE_PATH.
// Real React, fetch, texture decoding, WASM, and WebGPU run against local files, without servers or mocks.
test('asset failures leave the editor interactive and later loads retry successfully', { skip: process.env.EDITOR_ASSET_BROWSER_TESTS !== '1', timeout: 120000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const { build } = require('esbuild')
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-assets-browser-'))
    const meshDirectory = path.join(directory, 'data/mesh_data')
    const textureDirectory = path.join(directory, 'data/texture_data')
    let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined
    try {
        await mkdir(meshDirectory, { recursive: true })
        await mkdir(textureDirectory, { recursive: true })
        await copyFile(repoPath('tests/fixtures/asset-preview-mesh.bin'), path.join(meshDirectory, '1'))
        await copyFile(repoPath('tests/fixtures/asset-texture.bin'), path.join(textureDirectory, '1'))
        await writeFile(path.join(meshDirectory, '2'), new Uint8Array([0]))
        await writeFile(path.join(textureDirectory, '2'), new Uint8Array([0]))
        await mkdir(path.join(directory, 'basis'))
        for (const name of ['basis_transcoder.js', 'basis_transcoder.wasm']) await copyFile(repoPath('packages/website/lib/rendering/basis', name), path.join(directory, 'basis', name))
        // Expose Next's real Image component through ESM for esbuild's standalone browser bundle.
        const imageEntry = path.join(directory, 'next-image.mjs')
        await writeFile(imageEntry, `export { Image as default } from '${require.resolve('next/dist/client/image-component.js')}'`)
        const wasm = await readFile(repoPath('packages/website/lib/gen/render_web/render_web_bg.wasm'))
        const bundle = await build({
            stdin: { contents: `
                import React, { useReducer, useRef, useState } from 'react'
                import { createRoot } from 'react-dom/client'
                import { Toaster, toast } from 'sonner'
                import init, { BrowserWgpuRenderContext, BrowserWgpuRenderer } from '@/lib/gen/render_web/render_web'
                import { createWgpuRenderResources } from '@/lib/rendering/createWgpuRenderResources'
                import { EditorScene } from '@/app/editor/[game_id]/EditorScene'
                import { createEditorWorldState, editorWorldReducer } from '@/app/editor/editorWorld'
                import { AssetScenePreview } from '@/components/site/AssetScenePreview'
                import fixture from '${repoPath('tests/fixtures/asset-preview-object.json')}'
                await init({ module_or_path: Uint8Array.from(atob('${wasm.toString('base64')}'), c => c.charCodeAt(0)) })
                const world = structuredClone(fixture.Object)
                world.objectOfId[5] = { id: 5, name: 'Material', info: { Material: {
                    albedoTextureDataId: null, metallicTextureDataId: null, roughnessTextureDataId: null,
                    normalTextureDataId: null, isDoubleSided: false, alphaMode: 'Opaque',
                } } }
                world.childrenOfId[world.rootObjectId].push(5)
                world.childrenOfId[5] = []
                world.objectOfId[3].info.Ball.material = { materialInfoId: 5, tileSizeU: 1, tileSizeV: 1, tileOffsetU: 0, tileOffsetV: 0 }
                world.objectOfId[3].info.Ball.visualSettings.color = { r: 1, g: 1, b: 1 }
                const metrics = window.metrics = { draws: 0, uploads: [], camera: null, selected: null, selections: [] }
                // Observe real calls without changing their behavior or timing.
                for (const name of ['render', 'render_from_camera']) {
                    const render = BrowserWgpuRenderer.prototype[name]
                    BrowserWgpuRenderer.prototype[name] = function(...args) {
                        const result = render.apply(this, args)
                        metrics.draws++
                        if (name === 'render_from_camera') { metrics.camera = Array.from(args[3]); metrics.outlinedIds = Array.from(args[5]) }
                        return result
                    }
                }
                for (const kind of ['mesh', 'texture']) {
                    const name = 'load_' + kind + '_bytes_into_gpu'
                    const upload = BrowserWgpuRenderContext.prototype[name]
                    BrowserWgpuRenderContext.prototype[name] = function(id, bytes) {
                        const result = upload.call(this, id, bytes)
                        metrics.uploads.push({ kind, id: Number(id), draws: metrics.draws })
                        return result
                    }
                }
                const App = () => {
                    const [edited, setEdited] = useState(world)
                    const [state, dispatch] = useReducer(editorWorldReducer, world, createEditorWorldState)
                    metrics.selectedObjectIds = state.selectedObjectIds
                    const cameraRef = useRef(null)
                    window.selectObjects = ids => {
                        dispatch({ type: 'select', objectId: null })
                        for (const objectId of ids) dispatch({ type: 'toggle-select', objectId })
                    }
                    window.setAssets = (textureId, meshId, normalId = null) => setEdited(previous => {
                        const next = structuredClone(previous)
                        next.objectOfId[5].info.Material.albedoTextureDataId = textureId
                        next.objectOfId[5].info.Material.normalTextureDataId = normalId
                        next.objectOfId[4].info.Mesh.meshDataId = meshId
                        return next
                    })
                    window.retryAssets = () => setEdited(previous => {
                        const next = structuredClone(previous)
                        next.objectOfId[5].name += ' renamed'
                        return next
                    })
                    return <><Toaster /><EditorScene world={JSON.stringify(edited)} selectedObjectIds={state.selectedObjectIds} frameRequest={null} thumbnailUuid={null} cameraRef={cameraRef} firstDraftStatus={null} hasPlacedFirstDraftObject={false} onSelect={(id, isMultiSelect) => { metrics.selected = id; metrics.selections.push(id); metrics.isMultiSelect = isMultiSelect; dispatch({ type: isMultiSelect ? 'toggle-pick' : 'pick', objectId: id }) }} /></>
                }
                const root = createRoot(document.getElementById('root'))
                window.dismissErrors = () => toast.dismiss()
                window.unmount = () => root.render(null)
                window.mountAssets = () => {
                    const asset = structuredClone(world)
                    asset.objectOfId[5].info.Material.albedoTextureDataId = 2
                    asset.objectOfId[4].info.Mesh.meshDataId = 2
                    root.render(<><Toaster /><a href="#preview"><AssetScenePreview assetName="Invalid assets" assetDataJson={JSON.stringify({ Object: asset })} mode="card" /></a></>)
                }
                window.abortDownloads = async () => {
                    const controller = new AbortController()
                    const resources = await createWgpuRenderResources(controller.signal)
                    const loading = resources.loadAssetsIntoGpu(new BigInt64Array([1n]), new BigInt64Array([1n]))
                    controller.abort()
                    resources.browserWgpuRenderContext.free()
                    await loading
                }
                root.render(<App />)
            `, resolveDir: repoPath('packages/website'), loader: 'tsx' },
            bundle: true, write: false, format: 'esm', jsx: 'automatic',
            alias: { 'next/image': imageEntry },
            tsconfig: repoPath('packages/website/tsconfig.json'),
            define: {
                'process.env': '{}',
                'process.env.NODE_ENV': '"production"',
                'process.env.NEXT_PUBLIC_SUPABASE_URL': JSON.stringify(pathToFileURL(directory).href),
                'process.env.NEXT_PUBLIC_CDN_URL': JSON.stringify(pathToFileURL(directory).href),
                'process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY': '"unused-in-local-asset-test"',
            },
        })
        const html = path.join(directory, 'index.html')
        await writeFile(html, `<style>body{margin:0}#root{height:100vh;display:flex}#root>div,#root>a,#root>a>div{flex:1;position:relative}canvas{width:100%;height:100%;display:block}.invisible{visibility:hidden}.pointer-events-none{pointer-events:none}.absolute{position:absolute}.inset-0{inset:0}</style><div id="root"></div><script type="module">${bundle.outputFiles[0].text.replaceAll('</script', '<\\/script')}</script>`)
        browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
        const page = await browser.newPage({ viewport: { width: 800, height: 600 } })
        const errors: string[] = []
        const assetRequests: string[] = []
        page.on('pageerror', (error: Error) => errors.push(error.message))
        page.on('console', (message: { type: () => string, text: () => string }) => {
            if (message.type() === 'error' && !message.text().includes('net::ERR_FILE_NOT_FOUND')) errors.push(message.text())
        })
        page.on('request', (request: { url: () => string }) => { if (request.url().includes('/data/')) assetRequests.push(request.url()) })
        await page.goto(pathToFileURL(html).href)
        try { await page.waitForFunction('window.metrics?.draws > 2 && window.metrics.uploads.length === 1') }
        catch (error) { throw new Error(`${errors.join('\n')}\n${await page.locator('body').innerText()}`, { cause: error }) }
        assert.ok(await page.evaluate('window.metrics.uploads[0].draws > 0'), 'The editor must draw before its first asset upload')
        const canvas = page.getByLabel('Game world')

        // Multi-selection reaches the real WASM outline pass.
        await page.evaluate('window.selectObjects([3, 4])')
        await page.waitForFunction('window.metrics.outlinedIds?.length === 2')
        assert.deepEqual(await page.evaluate('window.metrics.outlinedIds'), [3, 4])
        await page.evaluate('window.selectObjects([])')
        await page.waitForFunction('window.metrics.outlinedIds?.length === 0')
        await canvas.click({ position: { x: 400, y: 300 } })
        await page.waitForFunction('window.metrics.selected !== null')

        const groupSelection = await page.evaluate('window.metrics.selectedObjectIds')
        for (const modifier of ['Shift', 'Meta']) {
            await page.keyboard.down(modifier)
            await canvas.click({ position: { x: 400, y: 300 } })
            await page.keyboard.up(modifier)
            assert.equal(await page.evaluate('window.metrics.isMultiSelect'), true)
            assert.deepEqual(await page.evaluate('window.metrics.selectedObjectIds'), [])
            await canvas.click({ position: { x: 400, y: 300 } })
            assert.deepEqual(await page.evaluate('window.metrics.selectedObjectIds'), groupSelection)
        }

        // Held keys (including repeat events) and keys pressed during a click must not cancel selection.
        const selectedObject = await page.evaluate('window.metrics.selected')
        await page.keyboard.down('Shift')
        for (const key of ['w', 's']) {
            const previousCamera = await page.evaluate('window.metrics.camera')
            const selections = await page.evaluate('window.metrics.selections.length')
            if (key === 'w') await page.keyboard.down(key)
            await page.mouse.down()
            await page.keyboard.down(key)
            await page.waitForFunction((previous: number[]) => window.metrics.camera.some((value: number, index: number) => value !== previous[index]), previousCamera)
            await page.mouse.up()
            assert.equal(await page.evaluate('window.metrics.selections.length'), selections + 1)
            assert.equal(await page.evaluate('window.metrics.selected'), selectedObject)
            const cameraAfterClick = await page.evaluate('window.metrics.camera')
            await page.waitForFunction((previous: number[]) => window.metrics.camera.some((value: number, index: number) => value !== previous[index]), cameraAfterClick)
            await page.keyboard.up(key)
        }
        await page.keyboard.up('Shift')

        await page.evaluate('window.setAssets(0, 0, 1)')
        const toasts = page.locator('[data-sonner-toast][data-type="error"]')
        await toasts.getByText('Could not load texture Data 0', { exact: true }).waitFor()
        await toasts.getByText('Could not load mesh Data 0', { exact: true }).waitFor()
        await page.waitForFunction('window.metrics.uploads.some(upload => upload.kind === "texture" && upload.id === 1)')
        const failedDraws = await page.evaluate('window.metrics.draws')
        await page.waitForFunction(`window.metrics.draws > ${failedDraws} + 2`)
        for (const kind of ['mesh', 'texture']) assert.equal(assetRequests.filter(url => url.endsWith('/' + kind + '_data/0')).length, 1, 'Drawing must not automatically retry failed downloads')
        const frameWithoutTexture = await canvas.evaluate((element: HTMLCanvasElement) => element.toDataURL())

        // Create the previously absent files. A later edit must issue fresh requests in the same GPU session.
        await copyFile(repoPath('tests/fixtures/asset-texture.bin'), path.join(textureDirectory, '0'))
        await copyFile(repoPath('tests/fixtures/asset-preview-mesh.bin'), path.join(meshDirectory, '0'))
        await page.evaluate('window.retryAssets()')
        await page.waitForFunction('window.metrics.uploads.filter(upload => upload.id === 0).length === 2')
        await page.waitForFunction((previous: string) => document.querySelector('canvas')!.toDataURL() !== previous, frameWithoutTexture)
        for (const kind of ['mesh', 'texture']) assert.ok(assetRequests.filter(url => url.endsWith('/' + kind + '_data/0')).length >= 2)

        // Corrupt bytes are contained just like failed fetches, and valid siblings still upload.
        await page.evaluate('window.setAssets(2, 2, 1)')
        await toasts.getByText('Could not load texture Data 2', { exact: true }).waitFor()
        await toasts.getByText('Could not load mesh Data 2', { exact: true }).waitFor()
        const beforeCamera = await page.evaluate('window.metrics.camera')
        await canvas.focus()
        await page.keyboard.down('w')
        await page.waitForFunction((previous: number[]) => window.metrics.camera.some((value: number, index: number) => value !== previous[index]), beforeCamera)
        await page.keyboard.up('w')
        assert.equal(assetRequests.filter(url => url.endsWith('/texture_data/1')).length, 1, 'Successful assets should not download again on every edit')

        await page.evaluate('window.dismissErrors()')
        await page.waitForFunction('document.querySelectorAll("[data-sonner-toast][data-type=error]").length === 0')
        const uploadCount = await page.evaluate('window.metrics.uploads.length')
        await page.evaluate('window.abortDownloads()')
        assert.equal(await page.evaluate('window.metrics.uploads.length'), uploadCount, 'Cancelled downloads must never upload into a freed GPU context')
        assert.equal(await toasts.count(), 0, 'Unmount cancellation is not a user-facing asset failure')

        // The shared loader also lets idle asset cards finish with missing/corrupt assets.
        const editorDraws = await page.evaluate('window.metrics.draws')
        await page.evaluate('window.mountAssets()')
        await toasts.getByText('Could not load texture Data 2', { exact: true }).waitFor()
        await page.waitForFunction(`window.metrics.draws > ${editorDraws}`)
        assert.equal(await page.getByRole('alert').count(), 0)

        // A failed decoder download must not poison future texture loads either.
        await rm(path.join(directory, 'basis/basis_transcoder.wasm'))
        await page.reload()
        await page.waitForFunction('window.metrics?.draws > 2 && window.metrics.uploads.length === 1')
        await page.evaluate('window.setAssets(1, 1)')
        await toasts.getByText('Could not load texture Data 1', { exact: true }).waitFor()
        await copyFile(repoPath('packages/website/lib/rendering/basis/basis_transcoder.wasm'), path.join(directory, 'basis/basis_transcoder.wasm'))
        await page.evaluate('window.retryAssets()')
        await page.waitForFunction('window.metrics.uploads.some(upload => upload.kind === "texture" && upload.id === 1)')
        assert.deepEqual(errors, [])
        await page.evaluate('window.unmount()')
    } finally {
        await browser?.close()
        await rm(directory, { recursive: true, force: true })
    }
})
