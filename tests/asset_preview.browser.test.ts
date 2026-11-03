import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdtemp, mkdir, readFile, writeFile, copyFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'

// Bundle a page with esbuild and write it next to the asset files it reads, so Chrome can open it from disk with no server.
const writePreviewPage = async (directory: string, source: string) => {
    const meshDirectory = path.join(directory, 'data/mesh_data')
    await mkdir(meshDirectory, { recursive: true })
    await copyFile(repoPath('tests/fixtures/asset-preview-mesh.bin'), path.join(meshDirectory, '1'))
    await copyFile(repoPath('tests/fixtures/asset-preview-animated-mesh.bin'), path.join(meshDirectory, '2'))
    const textureDirectory = path.join(directory, 'data/texture_data')
    await mkdir(textureDirectory, { recursive: true })
    await copyFile(repoPath('tests/fixtures/asset-texture.bin'), path.join(textureDirectory, '1'))
    // The production bundler emits these static assets beside the browser decoder module.
    await mkdir(path.join(directory, 'basis'))
    for (const name of ['basis_transcoder.js', 'basis_transcoder.wasm']) await copyFile(repoPath('packages/website/lib/rendering/basis', name), path.join(directory, 'basis', name))
    const { build } = createRequire(repoPath('packages/website/package.json'))('esbuild')
    const bundle = await build({
        stdin: { contents: source, resolveDir: repoPath('packages/website'), loader: 'tsx' },
        bundle: true, write: false, format: 'esm', jsx: 'automatic',
        tsconfig: repoPath('packages/website/tsconfig.json'),
        define: {
            'process.env.NODE_ENV': '"production"',
            'process.env.NEXT_PUBLIC_SUPABASE_URL': JSON.stringify(pathToFileURL(directory).href),
            'process.env.NEXT_PUBLIC_CDN_URL': JSON.stringify(pathToFileURL(directory).href),
            'process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY': '"unused-in-local-asset-test"',
        },
    })
    const html = path.join(directory, 'index.html')
    await writeFile(html, `<style>
        body{margin:0}.game-thumbnail{position:relative;width:100%;aspect-ratio:1.65;overflow:hidden}
        .absolute{position:absolute}.inset-0{inset:0}.pointer-events-none{pointer-events:none}.opacity-0{opacity:0}
        .sr-only{position:absolute;width:1px;height:1px;overflow:hidden}.bg-gradient-to-br{background:linear-gradient(to bottom right,#f3f4f6,#e5e7eb)}
        canvas{display:block;width:100%;aspect-ratio:1.65}.h-full{height:100%}.size-10{width:40px;height:40px}
    </style><div id="root"></div><script type="module">${bundle.outputFiles[0].text.replaceAll('</script', '<\\/script')}</script>`)
    return html
}

// Opt in with ASSET_BROWSER_TESTS=1 and Playwright available through NODE_PATH or website dependencies.
// This opens a local file in Chrome and exercises real React, WASM, WebGPU, observers, and asset reads; no server or mocks.
test('asset cards share GPU assets, render on demand, and retain independent frames', { skip: process.env.ASSET_BROWSER_TESTS !== '1', timeout: 120000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const directory = await mkdtemp(path.join(tmpdir(), 'asset-preview-browser-'))
    const wasm = await readFile(repoPath('packages/website/lib/gen/render_web/render_web_bg.wasm'))
    const html = await writePreviewPage(directory, `
            import React from 'react'
            import { createRoot } from 'react-dom/client'
            import init, { BrowserWgpuRenderContext, BrowserWgpuRenderer } from '@/lib/gen/render_web/render_web'
            import { AssetPreview } from '@/components/site/AssetPreview'
            import { AssetScenePreview } from '@/components/site/AssetScenePreview'
            import object from '${repoPath('tests/fixtures/asset-preview-object.json')}'
            import mesh from '${repoPath('tests/fixtures/asset-preview-mesh.json')}'
            import animatedMesh from '${repoPath('tests/fixtures/asset-preview-animated-mesh.json')}'
            import animatedObject from '${repoPath('tests/fixtures/asset-preview-animated-object.json')}'
            await init({ module_or_path: Uint8Array.from(atob('${wasm.toString('base64')}'), c => c.charCodeAt(0)) })
            // Instrument real methods without replacing their GPU or rendering behavior.
            const metrics = window.metrics = { created: 0, freed: 0, meshLoads: 0, textureLoads: [], textureUploadFormats: [], draws: {}, elapsed: {}, times: {} }
            const nameOfRenderer = new WeakMap()
            const gpuCanvasOfRenderContext = new WeakMap()
            const rendererOfName = window.rendererOfName = {}
            const renderArgsOfName = window.renderArgsOfName = {}
            const newRenderContext = BrowserWgpuRenderContext.new
            BrowserWgpuRenderContext.new = async (...args) => { metrics.created++; const browserWgpuRenderContext = await newRenderContext(...args); gpuCanvasOfRenderContext.set(browserWgpuRenderContext, args[0]); return browserWgpuRenderContext }
            const freeRenderContext = BrowserWgpuRenderContext.prototype.free
            BrowserWgpuRenderContext.prototype.free = function() { metrics.freed++; return freeRenderContext.call(this) }
            const loadMeshBytesIntoGpu = BrowserWgpuRenderContext.prototype.load_mesh_bytes_into_gpu
            BrowserWgpuRenderContext.prototype.load_mesh_bytes_into_gpu = function(...args) { metrics.meshLoads++; return loadMeshBytesIntoGpu.apply(this, args) }
            const loadTextureBytesIntoGpu = BrowserWgpuRenderContext.prototype.load_texture_bytes_into_gpu
            BrowserWgpuRenderContext.prototype.load_texture_bytes_into_gpu = function(id, bytes) { metrics.textureLoads.push(Array.from(bytes)); return loadTextureBytesIntoGpu.call(this, id, bytes) }
            const textureUploadFormat = BrowserWgpuRenderContext.prototype.texture_upload_format
            BrowserWgpuRenderContext.prototype.texture_upload_format = function(...args) { const uploadFormat = textureUploadFormat.apply(this, args); metrics.textureUploadFormats.push(uploadFormat); return uploadFormat }
            const rendererFromAssetDataJson = BrowserWgpuRenderer.from_asset_data_json
            BrowserWgpuRenderer.from_asset_data_json = (...args) => { const browserWgpuRenderer = rendererFromAssetDataJson(...args); nameOfRenderer.set(browserWgpuRenderer, args[2]); rendererOfName[args[2]] = browserWgpuRenderer; return browserWgpuRenderer }
            const render = BrowserWgpuRenderer.prototype.render
            BrowserWgpuRenderer.prototype.render = function(...args) {
                render.apply(this, args)
                const name = nameOfRenderer.get(this)
                renderArgsOfName[name] = args
                metrics.draws[name] = (metrics.draws[name] || 0) + 1
                metrics.elapsed[name] = args[3]
                metrics.times[name] ??= []
                metrics.times[name].push(args[3])
            }
            const root = createRoot(document.getElementById('root'))
            window.assetInputErrors = () => {
                const [browserWgpuRenderContext] = renderArgsOfName.Object
                return [
                    '{',
                    JSON.stringify({ Mesh: { ...mesh.Mesh, version: mesh.Mesh.version + 1 } }),
                    JSON.stringify({ Mesh: { version: mesh.Mesh.version, import_scale: 1, dehydrated_meshes: [], dehydrated_skeletons: [] } }),
                    JSON.stringify({ Sound: 1 }),
                    JSON.stringify({ Texture: 1 }),
                ].map(json => {
                    try {
                        const browserWgpuRenderer = BrowserWgpuRenderer.from_asset_data_json(browserWgpuRenderContext, json, 'Invalid')
                        browserWgpuRenderer.free()
                        return null
                    } catch (error) { return String(error) }
                })
            }
            window.meshDataInputErrors = async () => {
                const [browserWgpuRenderContext] = renderArgsOfName.Object
                const bytes = new Uint8Array(await (await fetch('./data/mesh_data/1')).arrayBuffer())
                const wrongVersion = new Uint8Array(8)
                new DataView(wrongVersion.buffer).setBigUint64(0, new DataView(bytes.buffer).getBigUint64(0, true) + 1n, true)
                return [new Uint8Array(), bytes.slice(0, 7), wrongVersion, bytes.slice(0, 8), bytes.slice(0, -1)].map(input => {
                    try {
                        loadMeshBytesIntoGpu.call(browserWgpuRenderContext, 1n, input)
                        return null
                    } catch (error) { return String(error) }
                })
            }
            const card = (name, type, data) => <a key={name} id={name} href={'#' + name} style={{display:'block', width:'40vw', maxWidth:320, margin:40}}><AssetPreview assetType={type} assetId={1} assetName={name} assetDataJson={data}/></a>
            const grid = showFirst => <>{showFirst && card('Object', 'Object', object)}{card('Mesh', 'Mesh', mesh)}<div style={{height:2000}}/>{card('Far', 'Object', object)}</>
            window.removeFirst = () => root.render(grid(false))
            window.removeAll = () => root.render(null)
            window.mountDetail = () => root.render(<div style={{width:420,margin:40}}><AssetScenePreview assetName="Detail" assetDataJson={JSON.stringify(object)} mode="detail" controls={null} footer={null}/></div>)
            window.mountAnimated = isObject => root.render(<div style={{width:420,margin:40}}><AssetScenePreview assetName="Animated" assetDataJson={JSON.stringify(isObject ? animatedObject : animatedMesh)} mode="detail" controls={null} footer={null}/></div>)
            const texturedMesh = alphaMode => ({ Mesh: { ...mesh.Mesh, dehydrated_meshes: mesh.Mesh.dehydrated_meshes.map(segment => ({ ...segment, material: { name: 'material', material_info: { albedoTextureDataId: 1, metallicTextureDataId: null, roughnessTextureDataId: null, normalTextureDataId: null, isDoubleSided: false, alphaMode } } })) } })
            window.mountTextured = alphaMode => root.render(<div style={{width:420,margin:40}}><AssetScenePreview assetName="Textured" assetDataJson={JSON.stringify(texturedMesh(alphaMode))} mode="detail" controls={null} footer={null}/></div>)
            window.samplePreviewFraming = () => {
                const [context, width, height, , yaw, pitch, zoom] = renderArgsOfName.Object
                const sample = (data, times) => {
                    const renderer = rendererFromAssetDataJson(context, JSON.stringify(data), 'Framing')
                    const canvas = document.createElement('canvas')
                    canvas.width = width; canvas.height = height
                    try {
                        return times.map(time => {
                            render.call(renderer, context, width, height, time, yaw, pitch, zoom)
                            canvas.getContext('2d').clearRect(0, 0, width, height)
                            canvas.getContext('2d').drawImage(gpuCanvasOfRenderContext.get(context), 0, 0)
                            return canvas.toDataURL()
                        })
                    } finally { renderer.free() }
                }
                const withCollider = structuredClone(object)
                const stored = withCollider.Object
                stored.objectOfId[5] = structuredClone(stored.objectOfId[3])
                stored.objectOfId[5].id = 5
                stored.objectOfId[5].info.Ball.visualSettings.opacity = 0
                stored.physicsInfo3DOfId[5] = structuredClone(stored.physicsInfo3DOfId[3])
                stored.physicsInfo3DOfId[5].location.position = [1000, 0, 0]
                stored.physicsToggles3DOfId[5] = structuredClone(stored.physicsToggles3DOfId[3])
                stored.primitiveSettingsOfId[5] = structuredClone(stored.primitiveSettingsOfId[3])
                stored.childrenOfId[stored.rootObjectId].push(5)
                stored.childrenOfId[5] = []
                const animated = structuredClone(object)
                animated.Object.objectOfId[3].info.Ball.visualSettings.opacity = 0
                for (const [id, info, children] of [
                    [5, { Clock: { clockState: { Playing: { localStartMs: 0, timeLocation: 'Client' } }, shouldLoop: false, speed: 1 } }, []],
                    [6, { PropertyAnimation: { duration: 1, interpolationStyle: 'Linear' } }, [7, 8]],
                    [7, { PropertyKeyframe: { time: 0, value: 0 } }, []],
                    [8, { PropertyKeyframe: { time: 1, value: 1 } }, []],
                    [9, { PropertyAnimator: { targetObjectId: 3, propertyPath: ['opacity'], propertyAnimationId: 6, clockId: 5, priority: 0, weight: 100, weightTransition: null } }, []],
                ]) {
                    animated.Object.objectOfId[id] = { id, name: 'Opacity animation', info }
                    animated.Object.childrenOfId[id] = children
                }
                animated.Object.childrenOfId[animated.Object.rootObjectId].push(5, 6, 9)
                const [hidden, revealed] = sample(animated, [0, 1000])
                return { baseline: sample(object, [0])[0], withCollider: sample(withCollider, [0])[0], hidden, revealed }
            }
            window.animatorTargetWeights = () => rendererOfName.Animated.sk_animators().map(animator => { const targetWeight = animator.target_weight; animator.free(); return targetWeight })
            window.sampleAnimationControls = () => {
                const browserWgpuRenderer = rendererOfName.Animated
                const animators = browserWgpuRenderer.sk_animators().map(animator => { const { animator_id, clock_id } = animator; animator.free(); return { animator_id, clock_id } })
                const [browserWgpuRenderContext, width, height, , yaw, pitch, zoom] = renderArgsOfName.Animated
                const previewCanvas = document.querySelector('canvas')
                const sample = (time, angle = yaw) => {
                    browserWgpuRenderer.render(browserWgpuRenderContext, width, height, time, angle, pitch, zoom)
                    previewCanvas.getContext('2d').clearRect(0, 0, width, height)
                    previewCanvas.getContext('2d').drawImage(gpuCanvasOfRenderContext.get(browserWgpuRenderContext), 0, 0)
                    return previewCanvas.toDataURL()
                }
                for (const [index, animator] of animators.entries()) browserWgpuRenderer.smoothly_change_weight(animator.animator_id, index % 2 === 0 ? 100 : 0, 0, 'Linear', 0)
                const [move, pose] = animators
                browserWgpuRenderer.play_clock(move.clock_id, 0, 0)
                browserWgpuRenderer.pause_clock(move.clock_id, 200)
                const paused = sample(200)
                const stillPaused = sample(800)
                browserWgpuRenderer.set_clock_speed(move.clock_id, 2, 800)
                browserWgpuRenderer.play_clock(move.clock_id, .2, 800)
                const playing = sample(900)
                browserWgpuRenderer.pause_clock(move.clock_id, 900)
                // Use the website's 250 ms selection fade, including its shared default easing.
                browserWgpuRenderer.select_sk_animator(pose.animator_id, 250, 1000)
                const start = sample(1000), middle = sample(1125), end = sample(1250)
                browserWgpuRenderer.select_sk_animator(move.animator_id, 250, 1125)
                const interrupted = sample(1125)
                sample(1125, yaw + .5)
                const afterOrbit = sample(1125)
                return { paused, stillPaused, playing, start, middle, end, interrupted, afterOrbit }
            }
            root.render(grid(true))
        `)
    const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
    try {
        const page = await browser.newPage({ viewport: { width: 1000, height: 600 }, deviceScaleFactor: 2 })
        const errors: string[] = []
        page.on('pageerror', (error: Error) => errors.push(error.message))
        page.on('console', (message: { type: () => string, text: () => string }) => { if (message.type() === 'error') errors.push(message.text()) })
        await page.goto(pathToFileURL(html).href)
        try {
            await page.waitForFunction(() => window.metrics?.draws.Object >= 1 && window.metrics?.draws.Mesh >= 1)
        } catch (error) {
            throw new Error(`${errors.join('\n')}\n${await page.locator('body').innerText()}`, { cause: error })
        }
        const inputErrors = await page.evaluate(() => window.assetInputErrors())
        assert.equal(inputErrors.length, 5)
        for (const error of inputErrors) assert.equal(typeof error, 'string', 'Invalid asset JSON must be rejected by from_asset_data_json')
        assert.match(inputErrors[1], /Unsupported dehydrated glTF version/)
        const meshInputErrors = await page.evaluate(() => window.meshDataInputErrors())
        for (const error of meshInputErrors) assert.equal(typeof error, 'string', 'Truncated mesh bytes must be rejected before changing GPU data')
        assert.match(meshInputErrors[2], /Unsupported data version/, 'Reject an obsolete version before decoding its missing payload')
        await page.waitForTimeout(150)
        assert.deepEqual(await page.evaluate(() => ({ created: window.metrics.created, meshLoads: window.metrics.meshLoads, draws: window.metrics.draws })), { created: 1, meshLoads: 1, draws: { Object: 1, Mesh: 1 } })
        // A 320 CSS-pixel card needs 640 backing pixels on a Retina display, including its shared-renderer output.
        assert.deepEqual(await page.locator('canvas').evaluateAll((canvases: HTMLCanvasElement[]) => canvases.slice(0, 2).map(canvas => [canvas.width, canvas.height])), [[640, 388], [640, 388]])
        // Distinct scenes must survive copying from a shared canvas, including shadows and a transparent import collider.
        const colors = await page.locator('canvas').evaluateAll((canvases: HTMLCanvasElement[]) => canvases.slice(0, 2).map(canvas => {
            const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data
            let red = 0, foreground = 0
            for (let i = 0; i < pixels.length; i += 4) {
                if (pixels[i] > pixels[i + 1] * 1.5 && pixels[i] > 40) red++
                if (Math.abs(pixels[i] - 243) + Math.abs(pixels[i + 1] - 243) + Math.abs(pixels[i + 2] - 243) > 30) foreground++
            }
            const bottomLeft = (canvas.height - 1) * canvas.width * 4
            return { red, foreground, skyTop: Array.from(pixels.slice(0, 3)), skyBottom: Array.from(pixels.slice(bottomLeft, bottomLeft + 3)) }
        }))
        assert.ok(colors[0].red > 100 && colors[1].red === 0 && colors[1].foreground > 100, JSON.stringify(colors))
        for (const color of colors) assert.notDeepEqual(color.skyTop, color.skyBottom, 'Asset previews should retain the sky gradient')
        const framing = await page.evaluate(() => window.samplePreviewFraming())
        assert.equal(framing.withCollider, framing.baseline, 'A permanent invisible collider must not move the camera or shrink the visible object')
        assert.notEqual(framing.hidden, framing.revealed, 'An initially hidden object must become visible through its opacity animation')
        assert.equal(framing.revealed, framing.baseline, 'Animated hidden geometry must retain the same framing when it appears')
        await page.locator('#Object').hover()
        await page.waitForFunction(() => window.metrics.draws.Object > 4 && window.metrics.elapsed.Object > 0)
        assert.equal(await page.evaluate(() => window.metrics.draws.Mesh), 1)
        await page.mouse.move(0, 0)
        await page.waitForFunction(() => window.metrics.elapsed.Object === 0)
        const stopped = await page.evaluate(() => window.metrics.draws.Object)
        await page.waitForTimeout(150)
        assert.equal(await page.evaluate(() => window.metrics.draws.Object), stopped)
        const restartIndex = await page.evaluate(() => window.metrics.times.Object.length)
        await page.locator('#Object').hover()
        await page.waitForFunction((index: number) => window.metrics.times.Object.length > index + 2, restartIndex)
        assert.equal(await page.evaluate((index: number) => window.metrics.times.Object[index], restartIndex), 0)
        assert.ok(await page.evaluate(() => window.metrics.elapsed.Object > 0))
        await page.mouse.move(0, 0)
        await page.waitForFunction(() => window.metrics.elapsed.Object === 0)
        await page.locator('#Mesh').focus()
        await page.waitForFunction(() => window.metrics.draws.Mesh > 4)
        await page.locator('#Mesh').evaluate((element: HTMLElement) => element.blur())
        await page.waitForFunction(() => window.metrics.elapsed.Mesh === 0)
        await page.evaluate(() => window.removeFirst())
        await page.waitForTimeout(100)
        assert.equal(await page.evaluate(() => window.metrics.freed), 0)
        await page.locator('#Far').scrollIntoViewIfNeeded()
        await page.mouse.move(0, 0)
        await page.waitForFunction(() => window.metrics.draws.Far >= 1)
        await page.waitForTimeout(100)
        const hiddenDraws = await page.evaluate(() => window.metrics.draws.Mesh)
        await page.waitForTimeout(150)
        assert.equal(await page.evaluate(() => window.metrics.draws.Mesh), hiddenDraws)
        assert.equal(await page.evaluate(() => window.metrics.meshLoads), 1)
        const beforeResize = await page.evaluate(() => window.metrics.draws.Far)
        await page.setViewportSize({ width: 700, height: 600 })
        await page.waitForFunction((previous: number) => window.metrics.draws.Far > previous, beforeResize)
        assert.equal(await page.locator('#Far canvas').evaluate((canvas: HTMLCanvasElement) => canvas.width), 560)
        await page.screenshot({ path: '/tmp/asset-grid-browser.png' })
        await page.evaluate(() => window.removeAll())
        await page.waitForFunction(() => window.metrics.freed === 1)
        await page.evaluate(() => { window.scrollTo(0, 0); window.mountDetail() })
        await page.waitForFunction(() => window.metrics.draws.Detail > 4)
        assert.equal(await page.evaluate(() => window.metrics.created), 2)
        const detail = page.locator('canvas')
        const beforeZoom = await detail.evaluate((canvas: HTMLCanvasElement) => canvas.toDataURL())
        await detail.hover()
        await page.mouse.wheel(0, 150)
        await page.waitForFunction((previous: string) => document.querySelector('canvas')!.toDataURL() !== previous, beforeZoom)
        await page.evaluate(() => window.removeAll())
        await page.waitForFunction(() => window.metrics.freed === 2)
        // Saved Object entries must expose and crossfade their animations just like Mesh entries.
        for (const [index, isObject] of [false, true].entries()) {
            const previousDraws = await page.evaluate(() => window.metrics.draws.Animated ?? 0)
            await page.evaluate((isObject: boolean) => window.mountAnimated(isObject), isObject)
            await page.waitForFunction((previous: number) => window.metrics.draws.Animated > previous + 4, previousDraws)
            assert.deepEqual(await page.evaluate(() => window.animatorTargetWeights()), [100, 0, 100, 0])
            // Duplicate clip names belong to distinct skeletons; clicking a clip must only fade its own skeleton.
            await page.getByRole('button', { name: 'Pose', exact: true }).nth(0).click()
            assert.deepEqual(await page.evaluate(() => window.animatorTargetWeights()), [0, 100, 100, 0])
            await page.locator('canvas').hover()
            await page.mouse.wheel(0, 100)
            await page.waitForTimeout(100)
            assert.deepEqual(await page.evaluate(() => window.animatorTargetWeights()), [0, 100, 100, 0])
            await page.getByRole('button', { name: 'Pose', exact: true }).nth(1).click()
            assert.deepEqual(await page.evaluate(() => window.animatorTargetWeights()), [0, 100, 0, 100])
            const frames = await page.evaluate(() => window.sampleAnimationControls())
            assert.equal(frames.paused, frames.stillPaused, 'Pause should hold the actual rendered pose')
            assert.notEqual(frames.paused, frames.playing, 'Changing speed and playing should update the pose')
            assert.notEqual(frames.start, frames.middle, 'A fade should produce an intermediate pose')
            assert.notEqual(frames.middle, frames.end, 'A fade should reach the target pose')
            assert.equal(frames.middle, frames.interrupted, 'Interrupting a fade should preserve its current pose')
            assert.equal(frames.middle, frames.afterOrbit, 'Orbiting should preserve edited animation state')
            await page.evaluate(() => window.removeAll())
            await page.waitForFunction((expected: number) => window.metrics.freed === expected, index + 3)
        }
        // Decode a real engine KTX2 upload in the browser and use every mip in the real WebGPU renderer.
        await page.evaluate(() => window.mountTextured('Opaque'))
        await page.waitForFunction(() => window.metrics.draws.Textured > 1)
        const textures = await page.evaluate(() => window.metrics.textureLoads)
        assert.equal(textures.length, 1)
        const texture = Uint8Array.from(textures[0])
        const header = new DataView(texture.buffer)
        assert.deepEqual([0, 4, 8, 12, 16].map(offset => header.getUint32(offset, true)), [16, 8, 5, 1, 1])
        // GPUs with texture compression get whole 16 byte blocks of 4x4 pixels; the rest get raw pixels.
        const uploadFormats = await page.evaluate(() => window.metrics.textureUploadFormats)
        assert.equal(uploadFormats.length, 1)
        if (uploadFormats[0] === 'rgba8unorm') {
            assert.equal(texture.length, 20 + (16 * 8 + 8 * 4 + 4 * 2 + 2 + 1) * 4)
            for (const { x, y, expected } of [{ x: 2, y: 2, expected: [255, 0, 0, 255] }, { x: 10, y: 2, expected: [0, 255, 0, 128] }, { x: 2, y: 6, expected: [0, 0, 255, 255] }]) {
                const offset = 20 + (y * 16 + x) * 4
                expected.forEach((channel, index) => assert.ok(Math.abs(texture[offset + index] - channel) <= 10))
            }
            assert.equal(texture[20 + (6 * 16 + 10) * 4 + 3], 0)
        } else {
            assert.ok(['bc7-rgba-unorm', 'astc-4x4-unorm'].includes(uploadFormats[0]))
            assert.equal(texture.length, 20 + (8 + 2 + 1 + 1 + 1) * 16)
        }
        const opaqueFrame = await page.locator('canvas').evaluate((canvas: HTMLCanvasElement) => canvas.toDataURL())
        const texturedDraws = await page.evaluate(() => window.metrics.draws.Textured)
        await page.evaluate(() => window.mountTextured('Blend'))
        await page.waitForFunction((previous: number) => window.metrics.draws.Textured > previous + 1, texturedDraws)
        const blendFrame = await page.locator('canvas').evaluate((canvas: HTMLCanvasElement) => canvas.toDataURL())
        assert.notEqual(blendFrame, opaqueFrame, 'Blend must use the uploaded texture alpha in the browser renderer')
        await page.evaluate(() => window.removeAll())
        await page.waitForFunction(() => window.metrics.freed === 6)
        assert.deepEqual(errors, [])
    } finally {
        await browser.close()
        await rm(directory, { recursive: true, force: true })
    }
})

// Chrome with --disable-gpu still has navigator.gpu but gives no adapter, like the browser in a user's bug report, so the
// page loads the WebGL2 build. Turning off the software rasterizer also turns off WebGL, so nothing on the page can draw.
test('previews stay grey and the page shows one message when the browser gives no GPU', { skip: process.env.ASSET_BROWSER_TESTS !== '1', timeout: 120000 }, async () => {
    const { chromium } = createRequire(repoPath('packages/website/package.json'))('playwright')
    const directory = await mkdtemp(path.join(tmpdir(), 'asset-preview-no-gpu-browser-'))
    const wasm = await readFile(repoPath('packages/website/lib/gen/render_web/render_web_bg.wasm'))
    const webGlWasm = await readFile(repoPath('packages/website/lib/gen/render_web_webgl/render_web_bg.wasm'))
    const html = await writePreviewPage(directory, `
        import React from 'react'
        import { createRoot } from 'react-dom/client'
        import { Toaster } from 'sonner'
        import init from '@/lib/gen/render_web/render_web'
        import initWebGl from '@/lib/gen/render_web_webgl/render_web'
        import { AssetPreview } from '@/components/site/AssetPreview'
        import { AssetScenePreview } from '@/components/site/AssetScenePreview'
        import object from '${repoPath('tests/fixtures/asset-preview-object.json')}'
        await init({ module_or_path: Uint8Array.from(atob('${wasm.toString('base64')}'), c => c.charCodeAt(0)) })
        await initWebGl({ module_or_path: Uint8Array.from(atob('${webGlWasm.toString('base64')}'), c => c.charCodeAt(0)) })
        const card = name => <a key={name} href={'#' + name} style={{display:'block', width:300, margin:20}}><AssetPreview assetType="Object" assetId={1} assetName={name} assetDataJson={object}/></a>
        createRoot(document.getElementById('root')).render(<>
            <Toaster/>
            {card('First')}{card('Second')}{card('Third')}
            <div style={{width:300, margin:20}}><AssetScenePreview assetName="Avatar" assetDataJson={JSON.stringify(object)} mode="spin" controls={null} footer={null}/></div>
        </>)
    `)
    const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files', '--disable-gpu', '--disable-software-rasterizer'] })
    try {
        const page = await browser.newPage({ viewport: { width: 1000, height: 1200 } })
        const errors: string[] = []
        page.on('pageerror', (error: Error) => errors.push(error.message))
        await page.goto(pathToFileURL(html).href)
        await page.waitForFunction(() => document.querySelectorAll('[role="status"]').length === 0 && document.querySelectorAll('[data-sonner-toast]').length > 0)
        await page.waitForTimeout(300)
        const toasts = await page.locator('[data-sonner-toast]').allInnerTexts()
        assert.equal(toasts.length, 1, 'The previews share one GPU setup, so its failure is one message for the page')
        assert.match(toasts[0], /3D previews could not start/)
        assert.match(toasts[0], /graphics acceleration/)
        assert.equal(await page.locator('[role="alert"]').count(), 0, 'Each card stays a grey tile with no error text of its own')
        assert.equal(await page.locator('canvas:not(.opacity-0)').count(), 0, 'An empty card canvas is opaque black, so it stays hidden over the grey tile')
        assert.deepEqual(errors, [])
    } finally {
        await browser.close()
        await rm(directory, { recursive: true, force: true })
    }
})

// Chrome with --disable-gpu gives WebGPU no adapter but still has WebGL2, so that page loads the WebGL2 build. Its previews
// must look like the WebGPU build's: the sky, ambient occlusion, glow, always-on-top objects, see-through layers, an image
// read both as color and as roughness, a skybox image, and a see-through background. Comparing the average of each 10 pixel square lets edge
// pixels differ, and lets the cutout differ in which pixels it draws, since WebGL2 draws cutouts as whole pixels.
test('the WebGL2 build draws the same previews as the WebGPU build', { skip: process.env.ASSET_BROWSER_TESTS !== '1', timeout: 180000 }, async () => {
    const { chromium } = createRequire(repoPath('packages/website/package.json'))('playwright')
    const directory = await mkdtemp(path.join(tmpdir(), 'asset-preview-webgl-browser-'))
    const wasm = await readFile(repoPath('packages/website/lib/gen/render_web/render_web_bg.wasm'))
    const webGlWasm = await readFile(repoPath('packages/website/lib/gen/render_web_webgl/render_web_bg.wasm'))
    const html = await writePreviewPage(directory, `
        import React from 'react'
        import { createRoot } from 'react-dom/client'
        import init from '@/lib/gen/render_web/render_web'
        import initWebGl from '@/lib/gen/render_web_webgl/render_web'
        import { AssetPreview } from '@/components/site/AssetPreview'
        import { AssetScenePreview } from '@/components/site/AssetScenePreview'
        import object from '${repoPath('tests/fixtures/asset-preview-object.json')}'
        import mesh from '${repoPath('tests/fixtures/asset-preview-mesh.json')}'
        await init({ module_or_path: Uint8Array.from(atob('${wasm.toString('base64')}'), c => c.charCodeAt(0)) })
        await initWebGl({ module_or_path: Uint8Array.from(atob('${webGlWasm.toString('base64')}'), c => c.charCodeAt(0)) })
        const ballWith = change => { const changed = structuredClone(object); change(changed.Object.objectOfId[3].info.Ball.visualSettings); return changed }
        const texturedMesh = (alphaMode, roughnessTextureDataId) => ({ Mesh: { ...mesh.Mesh, dehydrated_meshes: mesh.Mesh.dehydrated_meshes.map(segment => ({ ...segment, material: { name: 'material', material_info: { albedoTextureDataId: 1, metallicTextureDataId: null, roughnessTextureDataId, normalTextureDataId: null, isDoubleSided: true, alphaMode } } })) } })
        const card = (name, type, data) => <a key={name} href={'#' + name} style={{display:'inline-block', width:300, margin:10, verticalAlign:'top'}}><AssetPreview assetType={type} assetId={1} assetName={name} assetDataJson={data}/></a>
        createRoot(document.getElementById('root')).render(<>
            {card('Ball', 'Object', object)}
            {card('Mesh', 'Mesh', mesh)}
            {card('Glow', 'Object', ballWith(settings => { settings.glow = 1 }))}
            {card('OnTop', 'Object', ballWith(settings => { settings.isAlwaysOnTop = true }))}
            {card('Blend', 'Mesh', texturedMesh('Blend', null))}
            {card('Cutout', 'Mesh', texturedMesh({ Mask: { cutoff: 0.5 } }, null))}
            {card('Roughness', 'Mesh', texturedMesh('Opaque', 1))}
            {card('Skybox', 'Skybox', { Skybox: 1 })}
            <div style={{display:'inline-block', width:300, height:300, margin:10}}><AssetScenePreview assetName="Spin" assetDataJson={JSON.stringify(object)} mode="spin" controls={null} footer={null}/></div>
        </>)
    `)
    const drawPreviews = async (args: string[]) => {
        const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files', ...args] })
        try {
            const page = await browser.newPage({ viewport: { width: 1000, height: 900 }, deviceScaleFactor: 1 })
            const errors: string[] = []
            page.on('pageerror', (error: Error) => errors.push(error.message))
            await page.goto(pathToFileURL(html).href)
            await page.waitForFunction(() => document.querySelectorAll('canvas').length === 9 && document.querySelectorAll('[role="status"]').length === 0)
            assert.deepEqual(errors, [])
            assert.deepEqual(await page.locator('[role="alert"]').allInnerTexts(), [])
            assert.equal(await page.locator('canvas.opacity-0').count(), 0, 'Every preview drew')
            const hasWebGpuAdapter = await page.evaluate(async () => await navigator.gpu?.requestAdapter() !== null)
            const squareAverages = await page.locator('canvas').evaluateAll((canvases: HTMLCanvasElement[]) => canvases.map(canvas => {
                const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data
                const averages: number[] = []
                for (let top = 0; top + 10 <= canvas.height; top += 10) for (let left = 0; left + 10 <= canvas.width; left += 10) for (let channel = 0; channel < 4; channel++) {
                    let sum = 0
                    for (let y = top; y < top + 10; y++) for (let x = left; x < left + 10; x++) sum += pixels[(y * canvas.width + x) * 4 + channel]
                    averages.push(sum / 100)
                }
                return averages
            }))
            return { hasWebGpuAdapter, squareAverages }
        } finally {
            await browser.close()
        }
    }
    try {
        const webGpu = await drawPreviews([])
        const webGl = await drawPreviews(['--disable-gpu'])
        assert.ok(webGpu.hasWebGpuAdapter && !webGl.hasWebGpuAdapter, 'Only the browser with its GPU turned off should load the WebGL2 build')
        assert.equal(webGl.squareAverages.length, 9)
        for (const [index, averages] of webGl.squareAverages.entries()) {
            assert.equal(averages.length, webGpu.squareAverages[index].length)
            const largestDifference = Math.max(...averages.map((average, square) => Math.abs(average - webGpu.squareAverages[index][square])))
            // TODO: AI-PICKED-VALUE: The two builds differ by at most 3 of 255 in any 10 pixel square here, while a missing glow or see-through layer moves a square by 20 or more.
            assert.ok(largestDifference < 6, `Preview ${index} differs by ${largestDifference} in one 10 pixel square`)
        }
        // wgpu calls a WebGL2 canvas opaque, but the page must still show through the spin preview's empty corner.
        assert.equal(webGl.squareAverages[8][3], 0)
    } finally {
        await rm(directory, { recursive: true, force: true })
    }
})
