import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import { createRequire, registerHooks } from 'node:module'
import path from 'node:path'
import test from 'node:test'
import { createEditorWorldState, editorWorldReducer, type EditorWorldState, type EditorWorldAction } from '../packages/website/app/editor/editorWorld.ts'
import { objectInfoSchemaOfKind, storedWorldSchema } from '../packages/website/app/editor/editorObjectSchema.ts'
import type { EditorClientDelta } from '../packages/website/app/editor/editorSync.ts'
import { degreesToQuaternion, quaternionToDegrees } from '../packages/website/app/editor/editorRotation.ts'
import { repoPath } from './testPaths.ts'
import { withIsPushable } from './authoredWorld.ts'

const require = createRequire(new URL('../packages/website/package.json', import.meta.url))

// Mount the real panel and reducer in a DOM. No editor server, renderer, or network mocks are needed.
test('object panel browser interactions', async t => {
    const { JSDOM } = require('jsdom')
    const dom = new JSDOM('<!doctype html><html><body></body></html>', { pretendToBeVisual: true })
    const domGlobals = { window: dom.window, self: dom.window, document: dom.window.document, getComputedStyle: dom.window.getComputedStyle, HTMLElement: dom.window.HTMLElement, DocumentFragment: dom.window.DocumentFragment, HTMLSelectElement: dom.window.HTMLSelectElement, HTMLInputElement: dom.window.HTMLInputElement, Event: dom.window.Event, AbortController: dom.window.AbortController, requestAnimationFrame: dom.window.requestAnimationFrame, cancelAnimationFrame: dom.window.cancelAnimationFrame, IS_REACT_ACT_ENVIRONMENT: true }
    const previousDOMGlobals = Object.fromEntries(Object.keys(domGlobals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
    Object.assign(globalThis, domGlobals)
    const { act, createElement, useReducer, useLayoutEffect } = require('react')
    const { toast } = require('sonner')
    const { createRoot, hydrateRoot } = require('react-dom/client')
    const { renderToString } = require('react-dom/server')
    const directory = fs.mkdtempSync(repoPath('packages/website/.editor-panel-test-'))
    const ts = require('typescript')
    fs.writeFileSync(path.join(directory, 'package.json'), '{"type":"commonjs"}')
    const editorFiles = ['editorObjectSchema.ts', 'editorHistory.ts', 'editorWorld.ts', 'editorProperties.ts', 'editorRotation.ts', '[game_id]/EditorObjectIcon.tsx', '[game_id]/EditorObjectPanel.tsx', '[game_id]/EditorPanels.tsx', '[game_id]/EditorPropertyField.tsx', '[game_id]/EditorSidePanel.tsx', '[game_id]/EditorUploadPanel.tsx']
    const uiFiles = ['input', 'textarea', 'button', 'checkbox', 'select', 'tabs'].map(name => `shadcn/components/ui/${name}.tsx`)
    for (const file of [...editorFiles, ...uiFiles, 'shadcn/lib/utils.ts', 'lib/siteNavigation.ts']) {
        const output = path.join(directory, file.replace(/\.tsx?$/, '.js'))
        fs.mkdirSync(path.dirname(output), { recursive: true })
        const source = fs.readFileSync(repoPath(file.startsWith('shadcn/') || file.startsWith('lib/') ? 'packages/website' : 'packages/website/app/editor', file), 'utf8')
        const compiled = ts.transpileModule(source, { fileName: file, compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, esModuleInterop: true, rewriteRelativeImportExtensions: true } }).outputText
        fs.writeFileSync(output, compiled.replace(/require\("@\/([^"]+)"\)/g, (_match: string, specifier: string) => `require(${JSON.stringify('./' + path.relative(path.dirname(output), path.join(directory, specifier)))})`))
    }
    // Use the library's real browser entry so panel registration, resizing, and accessibility effects run.
    const hooks = registerHooks({
        resolve(specifier, context, nextResolve) {
            return nextResolve(specifier, specifier === 'react-resizable-panels' ? { ...context, conditions: ['development', 'browser'] } : context)
        },
    })
    const { EditorObjectPanel } = require(path.join(directory, '[game_id]/EditorObjectPanel.js'))
    const { EditorObjectIcon } = require(path.join(directory, '[game_id]/EditorObjectIcon.js'))
    const { EditorPanels } = require(path.join(directory, '[game_id]/EditorPanels.js'))
    const { EditorFileDropZone } = require(path.join(directory, '[game_id]/EditorSidePanel.js'))
    const { EditorUploadPanel } = require(path.join(directory, '[game_id]/EditorUploadPanel.js'))
    hooks.deregister()
    const world = storedWorldSchema.parse(withIsPushable(JSON.parse(fs.readFileSync(repoPath('game-35-latest.kube'), 'utf8'))))
    // This authored fixture predates the current MaterialInfo field; the editor server supplies it today.
    for (const object of Object.values(world.objectOfId)) {
        const material = object.info.Material
        if (material && typeof material === 'object' && !Array.isArray(material)) object.info.Material = { ...material, isDoubleSided: false }
        // It also predates the shadow softness, ambient occlusion, sky reflection and vibrance settings on WorldLighting.
        const worldLighting = object.info.WorldLighting
        if (worldLighting && typeof worldLighting === 'object' && !Array.isArray(worldLighting)) object.info.WorldLighting = { ...worldLighting, sunShadowSoftness: 2, ambientOcclusionIntensity: 0.7, ambientOcclusionRadius: 1, ambientOcclusionSunlightStrength: 0.5, skyReflectionStrength: 1, vibrance: 0.2 }
        // And it predates glow on every object's visual settings.
        for (const [kind, info] of Object.entries(object.info)) if (info && typeof info === 'object' && !Array.isArray(info) && 'visualSettings' in info && info.visualSettings && typeof info.visualSettings === 'object') object.info[kind] = { ...info, visualSettings: { ...info.visualSettings, glow: 0 } }
    }
    const brick = Object.values(world.objectOfId).find(object => object.info.Brick)
    assert.ok(brick)
    let latest: EditorWorldState = createEditorWorldState(world)
    let selectFromViewport: ((action: EditorWorldAction) => void) | undefined
    const framedObjectIds: number[] = []
    const edits: EditorClientDelta[] = []
    const Probe = () => {
        const [state, dispatchAction] = useReducer(editorWorldReducer, world, createEditorWorldState)
        const dispatch = (action: EditorWorldAction) => {
            if (action.type === 'edit') edits.push(...action.edits)
            dispatchAction(action)
        }
        useLayoutEffect(() => { latest = state; selectFromViewport = dispatch })
        const inspector = createElement(EditorObjectPanel, { editor: { state, dispatch, onSelect: (objectId: number, isMultiSelect: boolean) => dispatch({ type: isMultiSelect ? 'toggle-select' : 'select', objectId }), onFrame: (objectId: number) => framedObjectIds.push(objectId) } })
        return createElement(EditorPanels, { chat: createElement('textarea', { 'aria-label': 'Message' }), inspector }, createElement('canvas', { 'aria-label': 'Game world' }))
    }
    const container = document.createElement('div')
    document.body.appendChild(container)
    let root = createRoot(container)
    t.after(async () => {
        await act(async () => root.unmount())
        fs.rmSync(directory, { recursive: true, force: true })
        dom.window.close()
        for (const [key, descriptor] of Object.entries(previousDOMGlobals)) {
            if (descriptor) Object.defineProperty(globalThis, key, descriptor)
            else Reflect.deleteProperty(globalThis, key)
        }
    })
    const click = async (element: HTMLElement | null) => { assert.ok(element); await act(async () => element.click()) }
    const input = async (element: HTMLInputElement | null, value: string) => {
        assert.ok(element)
        await act(async () => {
            Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value')!.set!.call(element, value)
            element.dispatchEvent(new dom.window.Event('input', { bubbles: true }))
        })
    }
    // Exercise the native number input's step algorithm; jsdom does not synthesize browser default key actions.
    const step = async (element: HTMLInputElement | null, direction: number) => {
        assert.ok(element)
        await act(async () => element.focus())
        await act(async () => {
            if (direction > 0) element.stepUp()
            else element.stepDown()
            element.dispatchEvent(new dom.window.Event('input', { bubbles: true }))
        })
        await act(async () => element.blur())
    }
    const key = async (element: HTMLElement | null, value: string) => {
        assert.ok(element)
        await act(async () => element.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: value, bubbles: true })))
    }
    const selectBrick = async () => {
        await input(container.querySelector('input[aria-label="Search by name or id"]'), String(brick.id))
        await click(container.querySelector(`[data-object-id="${brick.id}"]`))
    }
    let mountId = 0
    const mount = async () => { mountId += 1; framedObjectIds.length = 0; edits.length = 0; await act(async () => root.render(createElement(Probe, { key: mountId }))) }

    await t.test('Shift, Command, and Control toggle Explorer rows and ordinary clicks replace the selection', async () => {
        await mount()
        const bricks = Object.values(world.objectOfId).filter(object => object.info.Brick).slice(0, 3)
        for (const [index, object] of bricks.entries()) {
            await input(container.querySelector('input[aria-label="Search by name or id"]'), String(object.id))
            const row = container.querySelector(`[data-object-id="${object.id}"]`)
            assert.ok(row)
            await act(async () => row.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true, shiftKey: index === 1, metaKey: index === 2 })))
            assert.deepEqual(latest.selectedObjectIds, bricks.slice(0, index + 1).map(object => object.id))
            assert.equal(row.getAttribute('aria-selected'), 'true')
        }
        const row = container.querySelector(`[data-object-id="${bricks[2].id}"]`)
        assert.ok(row)
        await act(async () => row.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true, ctrlKey: true })))
        assert.deepEqual(latest.selectedObjectIds, bricks.slice(0, 2).map(object => object.id))
        await input(container.querySelector('input[aria-label="Search by name or id"]'), String(bricks[0].id))
        await click(container.querySelector(`[data-object-id="${bricks[0].id}"]`))
        assert.deepEqual(latest.selectedObjectIds, [bricks[0].id])
        const first = container.querySelector(`[data-object-id="${bricks[0].id}"]`)
        assert.ok(first)
        await act(async () => first.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: ' ', bubbles: true, shiftKey: true })))
        assert.deepEqual(latest.selectedObjectIds, [])
    })

    await t.test('mixed properties set absolute values and preserve unedited position and rotation components', async () => {
        await mount()
        const [first, second] = Object.values(world.objectOfId).filter(object => object.info.Brick)
        const edited = structuredClone(world)
        for (const [index, object] of [first, second].entries()) {
            edited.physicsInfo3DOfId[object.id].location.position = index === 0 ? [1, 2, 3] : [11, 12, 13]
            edited.physicsInfo3DOfId[object.id].location.quaternion = degreesToQuaternion(index === 0 ? [10, 20, 30] : [40, 50, 60])
            edited.physicsToggles3DOfId[object.id].density = index === 0 ? 1 : 10
            edited.physicsToggles3DOfId[object.id].isPushable = index !== 0
        }
        await act(async () => {
            selectFromViewport?.({ type: 'refresh', world: edited })
            selectFromViewport?.({ type: 'select', objectId: first.id })
            selectFromViewport?.({ type: 'toggle-select', objectId: second.id })
        })
        const position = container.querySelector<HTMLInputElement>('input[aria-label="Position X"]')
        assert.ok(position)
        assert.equal(position.placeholder, 'Mixed')
        assert.equal(position.value, '')
        await act(async () => position.focus())
        await act(async () => position.blur())
        assert.deepEqual(edits, [], 'Focusing and blurring Mixed must not edit anything')
        await act(async () => position.focus())
        await input(position, '7')
        await key(position, 'Enter')
        assert.deepEqual(latest.world.physicsInfo3DOfId[first.id].location.position, [7, 2, 3])
        assert.deepEqual(latest.world.physicsInfo3DOfId[second.id].location.position, [7, 12, 13])
        const density = container.querySelector<HTMLInputElement>('input[aria-label="Density"]')
        assert.ok(density)
        await act(async () => density.focus())
        await input(density, '1')
        await key(density, 'Enter')
        assert.equal(latest.world.physicsToggles3DOfId[first.id].density, 1)
        assert.equal(latest.world.physicsToggles3DOfId[second.id].density, 1, 'The first selected value must also be assignable to all objects')
        const pushable = [...container.querySelectorAll('label')].find(label => label.textContent === 'Pushable')?.control
        assert.ok(pushable)
        assert.equal(pushable.getAttribute('aria-checked'), 'mixed')
        await click(pushable)
        assert.ok([first, second].every(object => latest.world.physicsToggles3DOfId[object.id].isPushable))
        const rotation = container.querySelector<HTMLInputElement>('input[aria-label="Quaternion X (degrees)"]')
        assert.ok(rotation)
        await act(async () => rotation.focus())
        await input(rotation, '70')
        await key(rotation, 'Enter')
        for (const [index, object] of [first, second].entries()) {
            const degrees = quaternionToDegrees(latest.world.physicsInfo3DOfId[object.id].location.quaternion)
            const expected = index === 0 ? [70, 20, 30] : [70, 50, 60]
            degrees.forEach((angle, axis) => assert.ok(Math.abs(angle - expected[axis]) < 1e-8))
        }
        assert.deepEqual(edits.slice(0, 2).map(edit => edit.objectId), [first.id, second.id], 'Both edits reach the same dispatch used by the sync queue')
        assert.equal(latest.error, null)
    })

    await t.test('every supported object kind renders a colored inline icon', () => {
        const iconOfKind: { [kind: string]: SVGSVGElement } = {}
        for (const kind of Object.keys(objectInfoSchemaOfKind)) {
            const markup = JSDOM.fragment(renderToString(createElement(EditorObjectIcon, { kind, className: 'size-3.5' })))
            const icon = markup.querySelector('svg')
            assert.ok(icon, kind)
            assert.ok(icon.querySelector('path, circle, line, rect, polygon, polyline, ellipse'), `${kind} must contain its icon geometry`)
            assert.match(icon.style.color, /^rgb\(/, `${kind} must retain its color independently of row selection`)
            assert.equal(icon.getAttribute('aria-hidden'), 'true')
            assert.equal(icon.getAttribute('focusable'), 'false')
            iconOfKind[kind] = icon
        }
        for (const [kind, icon] of Object.entries(iconOfKind)) {
            if (kind.startsWith('Sk')) assert.equal(icon.style.color, iconOfKind.Skeleton.style.color, kind)
            if (kind.startsWith('Property') && kind !== 'PropertyAnimator') assert.equal(icon.style.color, iconOfKind.PropertyAnimation.style.color, kind)
        }
        assert.notEqual(iconOfKind.PropertyAnimator.style.color, iconOfKind.PropertyAnimation.style.color)
        assert.equal(iconOfKind.SkJoint.innerHTML, iconOfKind.Skeleton.innerHTML, 'Joint fill must use the same bone geometry')
        assert.equal(iconOfKind.SkJoint.getAttribute('stroke-width'), iconOfKind.Skeleton.getAttribute('stroke-width'))
        assert.equal(iconOfKind.SkJoint.style.fill, 'currentcolor')
        assert.equal(iconOfKind.Skeleton.getAttribute('fill'), 'none')
    })

    await t.test('server markup hydrates with stable panel IDs and sizes, and the divider resizes', async () => {
        // Server rendering runs without a DOM and uses the package's server entry, as Next.js does.
        const server = spawnSync(process.execPath, ['--conditions=development', '-e', `
            const { createElement } = require('react')
            const { renderToString } = require('react-dom/server')
            const { EditorObjectPanel } = require('./[game_id]/EditorObjectPanel.js')
            const { EditorPanels } = require('./[game_id]/EditorPanels.js')
            process.stdout.write(renderToString(createElement(EditorPanels, { chat: 'Chat', inspector: createElement(EditorObjectPanel, { editor: null }) }, 'Viewport'), { identifierPrefix: 'server' }))
        `], { cwd: directory, encoding: 'utf8', timeout: 10000 })
        assert.equal(server.status, 0, server.stderr)
        assert.equal(server.stderr, '', 'Server rendering must not warn about missing default sizes')
        // Singleton panel identity must not depend on React's generated tree IDs.
        const panel = createElement(EditorPanels, { chat: 'Chat', inspector: createElement(EditorObjectPanel, { editor: null }) }, 'Viewport')
        assert.equal(renderToString(panel, { identifierPrefix: 'browser' }), server.stdout)
        await act(async () => root.unmount())
        container.innerHTML = server.stdout
        const panes = [...container.querySelectorAll<HTMLElement>('[data-panel]')]
        assert.equal(panes.length, 5)
        const initialSizes = panes.map(pane => Number(pane.style.flexGrow))
        for (const group of container.querySelectorAll('[data-panel-group]')) {
            const sizes = [...group.querySelectorAll<HTMLElement>(':scope > [data-panel]')].map(pane => Number(pane.style.flexGrow))
            assert.equal(sizes.reduce((sum, size) => sum + size, 0), 100)
        }
        const errors: unknown[] = []
        await act(async () => { root = hydrateRoot(container, panel, { identifierPrefix: 'browser', onRecoverableError: (error: unknown) => errors.push(error) }) })
        assert.deepEqual(errors, [])
        assert.deepEqual([...container.querySelectorAll('[data-panel]')], panes)
        assert.deepEqual(panes.map(pane => Number(pane.style.flexGrow)), initialSizes, 'Hydration must preserve the server layout')
        const divider = container.querySelector<HTMLElement>('[aria-label="Resize Explorer and Properties"]')
        assert.ok(divider)
        assert.equal(divider.getAttribute('aria-controls'), panes[3].id)
        await key(divider, 'ArrowDown')
        assert.ok(Number(panes[3].style.flexGrow) > initialSizes[3])
        assert.ok(Number(panes[4].style.flexGrow) < initialSizes[4])
    })

    await t.test('a side panel opens as a resizable column beside the viewport, keeps the other widths, and closing it restores the layout', async () => {
        const render = async (sidePanel: string | null) => act(async () => root.render(createElement(EditorPanels, { chat: 'Chat', sidePanel, inspector: 'Inspector' }, 'Viewport')))
        const sizeOfPanel = () => Object.fromEntries([...container.querySelectorAll<HTMLElement>('[data-panel-group-id="editor-workspace"] > [data-panel]')].map(pane => [pane.dataset.panelId, Number(pane.style.flexGrow)]))
        await render(null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 50, 'editor-sidebar': 25 })
        await render('Assets')
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 25, 'editor-side-panel': 25, 'editor-sidebar': 25 })
        const divider = container.querySelector<HTMLElement>('[aria-label="Resize viewport and side panel"]')
        assert.ok(divider)
        await key(divider, 'ArrowRight')
        assert.ok(sizeOfPanel()['editor-viewport'] > 25)
        assert.ok(sizeOfPanel()['editor-side-panel'] < 25)
        await render(null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 50, 'editor-sidebar': 25 })
        await key(container.querySelector('[aria-label="Resize chat and viewport"]'), 'ArrowRight')
        const chat = sizeOfPanel()['editor-chat']
        assert.ok(chat > 25)
        await render('Assets')
        assert.equal(sizeOfPanel()['editor-chat'], chat)
        assert.equal(sizeOfPanel()['editor-sidebar'], 25)
        await render(null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': chat, 'editor-viewport': 75 - chat, 'editor-sidebar': 25 })
    })

    // Chrome, like this DOM, has no FileSystemFileEntry or FileSystemDirectoryEntry globals, so a drop can only go by each entry's flags.
    await t.test('dropping files and a folder imports every file, including the ones inside the folder', async () => {
        const fileEntry = (name: string) => ({ isFile: true, isDirectory: false, file: (resolve: (file: File) => void) => resolve(new dom.window.File(['data'], name)) })
        let isFolderRead = false
        const folderEntry = { isFile: false, isDirectory: true, createReader: () => ({ readEntries: (resolve: (entries: unknown[]) => void) => { resolve(isFolderRead ? [] : [fileEntry('inside.png')]); isFolderRead = true } }) }
        let droppedNames: string[] = []
        await act(async () => root.render(createElement(EditorFileDropZone, { label: 'Drop files', accept: '', isMultiple: true, isDisabled: false, onFiles: (files: File[]) => { droppedNames = files.map(file => file.name) }, className: undefined })))
        const drop = new dom.window.Event('drop', { bubbles: true, cancelable: true })
        Object.defineProperty(drop, 'dataTransfer', { value: { items: [fileEntry('top.glb'), folderEntry].map(entry => ({ webkitGetAsEntry: () => entry })) } })
        await act(async () => { container.querySelector('label')?.dispatchEvent(drop) })
        assert.deepEqual(droppedNames, ['top.glb', 'inside.png'])
    })

    await t.test('the Skybox tab imports one panorama named after its file, and keeps it when the editor refuses it', async () => {
        const uploads: { kind: string, name: string }[] = []
        let refusal: Error | null = null
        const uploader = {
            uploadAsset: async (_file: File, kind: string, name: string) => {
                uploads.push({ kind, name })
                if (refusal !== null) throw refusal
                return 7
            },
            uploadMaterial: async () => { throw new Error('Only the Skybox tab is used here') },
        }
        await act(async () => root.render(createElement(EditorUploadPanel, { uploader, onClose: () => {} })))
        const skyboxTab = [...container.querySelectorAll<HTMLElement>('[role="tab"]')].find(tab => tab.textContent === 'Skybox')
        assert.ok(skyboxTab)
        await act(async () => { skyboxTab.dispatchEvent(new dom.window.MouseEvent('mousedown', { bubbles: true, button: 0 })) })
        const panel = container.querySelector<HTMLElement>('[role="tabpanel"][data-state="active"]')
        assert.ok(panel)
        const dropFile = async (name: string) => {
            const drop = new dom.window.Event('drop', { bubbles: true, cancelable: true })
            const entry = { isFile: true, isDirectory: false, file: (resolve: (file: File) => void) => resolve(new dom.window.File(['data'], name)) }
            Object.defineProperty(drop, 'dataTransfer', { value: { items: [{ webkitGetAsEntry: () => entry }] } })
            await act(async () => { panel.querySelector('label')?.dispatchEvent(drop) })
        }
        const importButton = () => [...panel.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'Import') ?? null

        await dropFile('Sunset Sky.png')
        await click(importButton())
        assert.deepEqual(uploads, [{ kind: 'Skybox', name: 'Sunset Sky' }])
        assert.ok(panel.querySelector('ul[aria-label="Imported skyboxes"] a[href="/assets/7"]'))
        assert.equal(importButton()?.disabled, true, 'An imported skybox should leave the form empty')

        refusal = new Error('A skybox must be a panorama twice as wide as it is tall, like 2048 x 1024. This image is 8 x 8.')
        await dropFile('Square.png')
        await click(importButton())
        assert.equal(panel.querySelector('[role="alert"]')?.textContent, refusal.message)
        assert.equal(panel.querySelector('label')?.textContent, 'Square.png')
        assert.equal(panel.querySelectorAll('ul[aria-label="Imported skyboxes"] li').length, 1)
    })

    await t.test('hiding the inspector gives its width to the viewport, and showing it again brings its width back', async () => {
        await act(async () => root.render(createElement('div')))
        const render = async (sidePanel: string | null, inspector: string | null) => act(async () => root.render(createElement(EditorPanels, { chat: 'Chat', sidePanel, inspector }, 'Viewport')))
        const sizeOfPanel = () => Object.fromEntries([...container.querySelectorAll<HTMLElement>('[data-panel-group-id="editor-workspace"] > [data-panel]')].map(pane => [pane.dataset.panelId, Number(pane.style.flexGrow)]))
        await render(null, null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 75 })
        assert.equal(container.querySelector('[aria-label="Resize viewport and inspector"]'), null)
        await render('Assets', null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 50, 'editor-side-panel': 25 })
        await render('Assets', 'Inspector')
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 25, 'editor-side-panel': 25, 'editor-sidebar': 25 })
        await key(container.querySelector('[aria-label="Resize viewport and inspector"]'), 'ArrowLeft')
        const sidebar = sizeOfPanel()['editor-sidebar']
        assert.ok(sidebar > 25)
        await render(null, null)
        assert.deepEqual(sizeOfPanel(), { 'editor-chat': 25, 'editor-viewport': 75 })
        await render(null, 'Inspector')
        assert.equal(sizeOfPanel()['editor-sidebar'], sidebar)
    })

    await t.test('viewport picking reveals each selected group and object without taking focus or moving the camera', async () => {
        await mount()
        const search = container.querySelector<HTMLInputElement>('input[aria-label="Search by name or id"]')
        assert.ok(search)
        search.focus()
        await input(search, 'No object matches this search')
        assert.equal(container.querySelector(`[data-object-id="${brick.id}"]`), null)
        const group = Object.values(world.objectOfId).find(object => object.info.RigidGroup && world.childrenOfId[object.id]?.includes(brick.id))
        assert.ok(group)
        for (const selected of [group, brick, brick]) {
            await act(async () => selectFromViewport?.({ type: 'pick', objectId: brick.id }))
            assert.equal((latest.selectedObjectIds.at(-1) ?? null), selected.id)
            assert.deepEqual(framedObjectIds, [], 'Viewport picking must not request camera framing')
            assert.equal(search.value, '')
            assert.equal(container.querySelector(`[data-object-id="${selected.id}"]`)?.getAttribute('aria-selected'), 'true')
            assert.equal(container.querySelector<HTMLInputElement>('section[aria-label="Properties"] input')!.value, selected.name)
            assert.equal(document.activeElement, search)
        }
        await act(async () => selectFromViewport?.({ type: 'pick', objectId: null }))
        assert.match(container.querySelector('section[aria-label="Properties"]')!.textContent!, /Select an object/)
    })

    await t.test('selection highlights descendants through nested and filtered rows, then clears when selection changes', async () => {
        await mount()
        await click(container.querySelector(`[data-object-id="${world.rootObjectId}"]`))
        assert.ok(container.querySelector('[role="treeitem"][aria-level="3"]'), 'Exercise grandchildren as well as direct children')
        const assertSubtree = () => {
            for (const row of container.querySelectorAll<HTMLElement>('[role="treeitem"]')) {
                const isRoot = Number(row.dataset.objectId) === world.rootObjectId
                assert.ok(row.classList.contains(isRoot ? 'bg-blue-100' : 'bg-blue-50'))
                assert.equal(row.getAttribute('aria-selected'), String(isRoot), 'The parent remains the primary object being inspected')
            }
        }
        assertSubtree()
        await input(container.querySelector('input[aria-label="Search by name or id"]'), String(brick.id))
        assertSubtree()
        await click(container.querySelector(`[data-object-id="${brick.id}"]`))
        assert.equal((latest.selectedObjectIds.at(-1) ?? null), brick.id)
        for (const row of container.querySelectorAll<HTMLElement>('[role="treeitem"]')) {
            assert.equal(row.classList.contains('bg-blue-100'), Number(row.dataset.objectId) === brick.id)
            assert.equal(row.classList.contains('bg-blue-50'), false, 'Ancestors do not inherit a child selection')
        }
        await act(async () => selectFromViewport?.({ type: 'select', objectId: null }))
        assert.equal(container.querySelector('[role="treeitem"].bg-blue-100, [role="treeitem"].bg-blue-50'), null)
    })

    await t.test('only double-clicking an Explorer row frames the object', async () => {
        await mount()
        await selectBrick()
        const row = container.querySelector<HTMLElement>(`[data-object-id="${brick.id}"]`)
        assert.ok(row)
        await click(row)
        await key(row, 'Enter')
        await key(row, ' ')
        assert.equal((latest.selectedObjectIds.at(-1) ?? null), brick.id)
        assert.deepEqual(framedObjectIds, [], 'Single clicks and keyboard selection must leave the camera in place')
        for (let repeat = 0; repeat < 2; repeat++) {
            await click(row)
            await click(row)
            await act(async () => row.dispatchEvent(new dom.window.MouseEvent('dblclick', { bubbles: true, detail: 2 })))
        }
        assert.deepEqual(framedObjectIds, [brick.id, brick.id], 'Double-clicking the selected row must reframe it')
        await input(container.querySelector('input[aria-label="Search by name or id"]'), '')
        const disclosure = container.querySelector<HTMLButtonElement>(`[data-object-id="${world.rootObjectId}"] button`)
        assert.ok(disclosure)
        await click(disclosure)
        await click(disclosure)
        await act(async () => disclosure.dispatchEvent(new dom.window.MouseEvent('dblclick', { bubbles: true, detail: 2 })))
        assert.deepEqual(framedObjectIds, [brick.id, brick.id], 'Expanding the tree must not frame its parent')
    })

    await t.test('both side dividers resize without losing selection or chat text', async () => {
        await mount()
        await selectBrick()
        const message = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!
        message.value = 'Keep this draft'
        for (const [label, panelId, arrow] of [['Resize chat and viewport', 'editor-chat', 'ArrowRight'], ['Resize viewport and inspector', 'editor-sidebar', 'ArrowLeft']]) {
            const pane = container.querySelector<HTMLElement>(`[data-panel-id="${panelId}"]`)!
            const before = Number(pane.style.flexGrow)
            await key(container.querySelector(`[aria-label="${label}"]`), arrow)
            assert.notEqual(Number(pane.style.flexGrow), before)
        }
        assert.equal(container.querySelector('textarea[aria-label="Message"]'), message)
        assert.equal(message.value, 'Keep this draft')
        assert.equal((latest.selectedObjectIds.at(-1) ?? null), brick.id)
    })

    await t.test('search, selection, renaming, and switching selection retain React values', async () => {
        await mount()
        await selectBrick()
        assert.equal((latest.selectedObjectIds.at(-1) ?? null), brick.id)
        const name = container.querySelector<HTMLInputElement>('section[aria-label="Properties"] input')
        await input(name, 'Renamed-in-browser')
        assert.equal(latest.world.objectOfId[brick.id].name, 'Renamed-in-browser')
        assert.match(container.querySelector(`[data-object-id="${brick.id}"]`)!.textContent!, /Renamed-in-browser/)
        await click(container.querySelector(`[data-object-id="${world.rootObjectId}"]`))
        await click(container.querySelector(`[data-object-id="${brick.id}"]`))
        assert.equal(container.querySelector<HTMLInputElement>('section[aria-label="Properties"] input')!.value, 'Renamed-in-browser')
        assert.equal(world.objectOfId[brick.id].name, brick.name)
    })

    await t.test('numeric edits update the world while focused; unfinished input stays local', async () => {
        await mount()
        await selectBrick()
        const position = container.querySelector<HTMLInputElement>('input[aria-label="Position X"]')
        assert.ok(position)
        const original = latest.world.physicsInfo3DOfId[brick.id].location.position[0]
        await act(async () => position.focus())
        await input(position, '-')
        assert.equal(position.getAttribute('aria-invalid'), 'false', 'Do not show an error while a number is unfinished')
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], original)
        await act(async () => position.blur())
        assert.equal(position.getAttribute('aria-invalid'), 'true')
        assert.equal(toast.getHistory().find((entry: { id: string }) => entry.id === 'editor-property-error')?.title, 'Position X: Enter a finite number.')
        assert.equal(container.querySelector('[role="alert"]'), null)
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], original)
        await key(position, 'Escape')
        assert.equal(position.getAttribute('aria-invalid'), 'false')
        await act(async () => position.focus())
        await input(position, '-12.5')
        assert.equal(document.activeElement, position)
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], -12.5)
        assert.deepEqual(edits.at(-1)?.value, latest.world.physicsInfo3DOfId[brick.id].location.position)
        const editCount = edits.length
        await key(position, 'Enter')
        assert.equal(edits.length, editCount, 'Blur must not submit a number again after typing already applied it')
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], -12.5)
        const opacity = container.querySelector<HTMLInputElement>('input[aria-label="Opacity"]')
        assert.ok(opacity)
        await act(async () => opacity.focus())
        await input(opacity, '2')
        await act(async () => opacity.blur())
        assert.equal(opacity.getAttribute('aria-invalid'), 'false')
        assert.equal(opacity.value, '1')
        await click(container.querySelector(`[data-object-id="${world.rootObjectId}"]`))
        await click(container.querySelector(`[data-object-id="${brick.id}"]`))
        assert.equal(container.querySelector('input[aria-label="Opacity"]')!.getAttribute('aria-invalid'), 'false')
    })

    await t.test('live numeric edits clamp without changing the typed text, snapping to the step, or adding unchanged edits', async () => {
        await mount()
        await selectBrick()
        const opacity = container.querySelector<HTMLInputElement>('input[aria-label="Opacity"]')
        const size = container.querySelector<HTMLInputElement>('input[aria-label="Size X"]')
        assert.ok(opacity && size)
        const cases: [HTMLInputElement, string, number][] = [[opacity, '-10', 0], [opacity, '7', 1], [size, '-9', 0], [opacity, '0.1234', 0.1234]]
        for (const [field, draft, expected] of cases) {
            await act(async () => field.focus())
            await input(field, draft)
            assert.equal(field.value, draft, 'Typing must keep the entered number until commit')
            const editedValue = edits.at(-1)?.value
            assert.equal(Array.isArray(editedValue) ? editedValue[0] : editedValue, expected, 'The normalized number must be applied before blur')
            const previousWorld = latest.world
            await key(field, 'Enter')
            assert.equal(latest.world, previousWorld, 'Blur must not repeat the live edit')
            assert.equal(field.getAttribute('aria-invalid'), 'false')
            await act(async () => field.focus())
            assert.equal(Number(field.value), expected)
            await act(async () => field.blur())
        }
        await act(async () => opacity.focus())
        await input(opacity, '9')
        await key(opacity, 'Escape')
        await act(async () => opacity.blur())
        assert.equal(edits.at(-1)?.value, 1, 'Escape clears the text without undoing an edit that was already applied')
        await act(async () => opacity.focus())
        await input(opacity, '2')
        await act(async () => opacity.blur())
        assert.equal(opacity.value, '1')
        const previousWorld = latest.world
        await act(async () => opacity.focus())
        await input(opacity, '1000')
        await act(async () => opacity.blur())
        assert.equal(latest.world, previousWorld, 'Clamping to the current value must not add an edit')
    })

    await t.test('rejected property edits report a toast without changing the world or adding an error bar', async () => {
        await mount()
        await selectBrick()
        const previousWorld = latest.world
        await act(async () => selectFromViewport!({ type: 'edit', description: 'Test edit', edits: [{ objectId: brick.id, section: 'objectOfId', path: ['id'], value: 0 }] }))
        assert.equal(latest.world, previousWorld)
        assert.equal(toast.getHistory().find((entry: { id: string }) => entry.id === 'editor-property-error')?.title, 'This property cannot be edited.')
        assert.equal(container.querySelector('[role="alert"]'), null)
    })

    await t.test('compact numbers retain full precision through focus, blur, and cancel', async () => {
        await mount()
        await selectBrick()
        const position = container.querySelector<HTMLInputElement>('input[aria-label="Position X"]')
        assert.ok(position)
        for (const value of [24.290412345, 0.000000123456789]) {
            await act(async () => position.focus())
            await input(position, String(value))
            await key(position, 'Enter')
            assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], value)
            assert.notEqual(Number(position.value), 0, 'Small nonzero values must not appear as zero')
            assert.notEqual(position.value, String(value), 'Unfocused numbers should be compact')
            const previousWorld = latest.world
            await act(async () => position.focus())
            assert.equal(position.value, String(value))
            await act(async () => position.blur())
            assert.equal(latest.world, previousWorld, 'An unchanged blur must not round stored data')
            await act(async () => position.focus())
            await input(position, '-')
            await key(position, 'Escape')
            await act(async () => position.blur())
            assert.equal(latest.world, previousWorld, 'Cancel must retain the original precision')
        }
    })

    await t.test('native number inputs step by 0.05 and respect field limits', async () => {
        await mount()
        await selectBrick()
        const position = container.querySelector<HTMLInputElement>('input[aria-label="Position X"]')
        assert.ok(position)
        await act(async () => position.focus())
        await input(position, '12.5')
        await key(position, 'Enter')
        await step(position, 1)
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], 12.55)
        await step(position, -1)
        assert.equal(latest.world.physicsInfo3DOfId[brick.id].location.position[0], 12.5)
        const opacity = container.querySelector<HTMLInputElement>('input[aria-label="Opacity"]')
        assert.ok(opacity)
        for (const [draft, direction, bound] of [[0.95, 1, 1], [0.05, -1, 0]]) {
            await act(async () => opacity.focus())
            await input(opacity, String(draft))
            await key(opacity, 'Enter')
            await step(opacity, direction)
            const info = latest.world.objectOfId[brick.id].info.Brick
            assert.ok(info && typeof info === 'object' && !Array.isArray(info))
            const visual = info.visualSettings
            assert.ok(visual && typeof visual === 'object' && !Array.isArray(visual))
            assert.equal(visual.opacity, bound)
            const previousWorld = latest.world
            await step(opacity, direction)
            assert.equal(latest.world, previousWorld, 'Stepping past a bound must not add an edit')
        }
    })

    await t.test('integer fields have no steppers and negative-degree fields step in viewing units', async () => {
        await mount()
        const animator = Object.values(world.objectOfId).find(object => object.info.PropertyAnimator)
        assert.ok(animator)
        await act(async () => selectFromViewport?.({ type: 'select', objectId: animator.id }))
        const priority = container.querySelector<HTMLInputElement>('input[aria-label="Priority"]')
        assert.ok(priority)
        assert.equal(priority.type, 'text')
        assert.equal(priority.inputMode, 'numeric')
        await act(async () => priority.focus())
        await input(priority, '2.5')
        assert.equal(edits.at(-1)?.value, 3, 'Integer rounding applies while typing')
        assert.equal(priority.value, '2.5', 'Keep the typed decimal until blur')
        await key(priority, 'Enter')
        assert.equal(priority.getAttribute('aria-invalid'), 'false')
        assert.equal(edits.at(-1)?.value, 3)
        for (const [draft, expected] of [['100', 10], ['-10', 0]]) {
            await act(async () => priority.focus())
            await input(priority, String(draft))
            await key(priority, 'Enter')
            assert.equal(edits.at(-1)?.value, expected)
        }
        const lighting = Object.values(world.objectOfId).find(object => object.info.WorldLighting)
        assert.ok(lighting)
        await act(async () => selectFromViewport?.({ type: 'select', objectId: lighting.id }))
        const phi = container.querySelector<HTMLInputElement>('input[aria-label="Phi"]')
        assert.ok(phi)
        await act(async () => phi.focus())
        await input(phi, '30')
        const livePhi = edits.at(-1)?.value
        assert.ok(typeof livePhi === 'number' && Math.abs(livePhi + 30 * Math.PI / 180) < 1e-10, 'Degree conversion applies while typing')
        await key(phi, 'Enter')
        await step(phi, 1)
        const edit = edits.at(-1)
        assert.deepEqual(edit?.path, ['info', 'WorldLighting', 'sunOrientation', 'phi'])
        assert.ok(edit && typeof edit.value === 'number')
        assert.ok(Math.abs(edit.value + 30.05 * Math.PI / 180) < 1e-10)
        await act(async () => phi.focus())
        await input(phi, '1e100')
        await key(phi, 'Enter')
        assert.equal(phi.getAttribute('aria-invalid'), 'false')
        assert.equal(Number(phi.value), Number(phi.max), 'Inverted degree fields must clamp in stored units and display the corresponding bound')
    })

    await t.test('Transform and Movement rows preserve storage paths and degree conversion', async () => {
        await mount()
        await selectBrick()
        const velocity = container.querySelector<HTMLInputElement>('input[aria-label="Velocity Y"]')
        const angularVelocity = container.querySelector<HTMLInputElement>('input[aria-label="Angular Velocity Z"]')
        const rotation = container.querySelector<HTMLInputElement>('input[aria-label="Quaternion Y (degrees)"]')
        assert.ok(velocity && angularVelocity && rotation)
        const changes: [HTMLInputElement, string][] = [[velocity, '2.75'], [angularVelocity, '90'], [rotation, '82.5']]
        for (const [field, value] of changes) {
            await act(async () => field.focus())
            await input(field, value)
            await key(field, 'Enter')
        }
        const physics = latest.world.physicsInfo3DOfId[brick.id]
        assert.equal(physics.movement.velocity[1], 2.75)
        assert.ok(Math.abs(physics.movement.angularVelocity[2] - Math.PI / 2) < 1e-10)
        assert.ok(edits.some(edit => edit.section === 'physicsInfo3DOfId' && edit.path.join('.') === 'location.quaternion'))
        const previousWorld = latest.world
        await act(async () => rotation.focus())
        await act(async () => rotation.blur())
        assert.equal(latest.world, previousWorld, 'Inspecting an angle must not round its quaternion')
        await step(angularVelocity, 1)
        assert.ok(Math.abs(latest.world.physicsInfo3DOfId[brick.id].movement.angularVelocity[2] - 90.05 * Math.PI / 180) < 1e-10)
        await step(rotation, 1)
        assert.ok(Math.abs(Number(rotation.value) - 82.55) < 1e-10)
    })

    await t.test('optional numeric values share one label and row while editing and toggling', async () => {
        await mount()
        const material = Object.values(world.objectOfId).find(object => object.info.Material)
        assert.ok(material)
        await act(async () => selectFromViewport?.({ type: 'select', objectId: material.id }))
        const properties = container.querySelector<HTMLElement>('section[aria-label="Properties"]')!
        const enable = properties.querySelector<HTMLButtonElement>('[role="checkbox"][aria-label="Enable Albedo Texture Data Id"]')
        assert.ok(enable)
        if (enable.getAttribute('aria-checked') !== 'true') await click(enable)
        const field = properties.querySelector<HTMLInputElement>('input[aria-label="Albedo Texture Data Id"]')
        assert.ok(field)
        assert.equal(field.type, 'text')
        assert.equal(field.inputMode, 'numeric')
        assert.equal(properties.textContent!.split('Albedo Texture Data Id').length - 1, 1)
        assert.equal(field.closest('.grid'), enable.closest('.grid'), 'The checkbox and value belong to the same property row')
        await act(async () => field.focus())
        await input(field, '361')
        await key(field, 'Enter')
        assert.deepEqual(edits.at(-1)?.path, ['info', 'Material', 'albedoTextureDataId'])
        assert.equal(edits.at(-1)?.value, 361)
        await click(enable)
        assert.equal(edits.at(-1)?.value, null)
        assert.equal(properties.querySelector('input[aria-label="Albedo Texture Data Id"]'), null)
        assert.equal(properties.textContent!.split('Albedo Texture Data Id').length - 1, 1)
        await click(properties.querySelector('[role="checkbox"][aria-label="Enable Albedo Texture Data Id"]'))
        assert.equal(properties.querySelector<HTMLInputElement>('input[aria-label="Albedo Texture Data Id"]')?.value, '0')
    })

    await t.test('boolean fields update immediately and object references reveal their target', async () => {
        await mount()
        await selectBrick()
        const pushable = [...container.querySelectorAll('label')].find(label => label.textContent === 'Pushable')?.control
        assert.ok(pushable)
        const previous = latest.world.physicsToggles3DOfId[brick.id].isPushable
        await click(pushable)
        assert.equal(latest.world.physicsToggles3DOfId[brick.id].isPushable, !previous)
        const enable = container.querySelector<HTMLButtonElement>('[role="checkbox"][aria-label="Enable Material"]')
        assert.ok(enable)
        if (enable.getAttribute('aria-checked') !== 'true') await click(enable)
        assert.equal([...container.querySelectorAll('section[aria-label="Properties"] label, section[aria-label="Properties"] legend')].filter(label => label.textContent === 'Material').length, 1, 'An optional object should not repeat its heading')
        const link = container.querySelector<HTMLButtonElement>('button[aria-label="Select Material Info Id object"]')
        assert.ok(link)
        await click(link)
        assert.notEqual((latest.selectedObjectIds.at(-1) ?? null), brick.id)
        assert.ok((latest.selectedObjectIds.at(-1) ?? null) !== null && latest.world.objectOfId[(latest.selectedObjectIds.at(-1) ?? null)].info.Material)
        assert.deepEqual(framedObjectIds, [], 'Following a property reference must not move the camera')
        assert.equal(container.querySelector<HTMLInputElement>('input[aria-label="Search by name or id"]')!.value, '')
        assert.ok(container.querySelector(`[data-object-id="${(latest.selectedObjectIds.at(-1) ?? null)}"][aria-selected="true"]`))
    })

    await t.test('the color picker sets the whole color', async () => {
        await mount()
        await selectBrick()
        const picker = container.querySelector<HTMLInputElement>('input[aria-label="Color picker"]')
        assert.ok(picker)
        await input(picker, '#ff8000')
        assert.deepEqual(edits.at(-1)?.path, ['info', 'Brick', 'visualSettings', 'color'])
        assert.deepEqual(edits.at(-1)?.value, { r: 1, g: 128 / 255, b: 0 })
    })

    await t.test('tree arrow keys move focus and Enter selects the focused object', async () => {
        await mount()
        const first = container.querySelector<HTMLElement>('[role="treeitem"]')
        assert.ok(first)
        await act(async () => first.focus())
        await key(first, 'ArrowDown')
        assert.notEqual(document.activeElement, first)
        const focused = document.activeElement
        assert.ok(focused instanceof dom.window.HTMLElement)
        await key(focused, 'Enter')
        assert.equal(String((latest.selectedObjectIds.at(-1) ?? null)), focused.getAttribute('data-object-id'))
        await key(focused, 'Home')
        assert.equal(document.activeElement, first)
        const selectedObjectId = (latest.selectedObjectIds.at(-1) ?? null)
        await key(first, 'ArrowLeft')
        assert.equal(first.getAttribute('aria-expanded'), 'false')
        await key(first, 'ArrowRight')
        assert.equal(first.getAttribute('aria-expanded'), 'true')
        await key(first, 'ArrowDown')
        await key(document.activeElement, 'ArrowUp')
        assert.equal(document.activeElement, first)
        assert.equal((latest.selectedObjectIds.at(-1) ?? null), selectedObjectId)
        assert.deepEqual(framedObjectIds, [], 'Tree navigation and expansion must never request camera framing')
    })

    await t.test('enum controls preserve Rust variant shapes when entering and leaving a data variant', async () => {
        await mount()
        const material = Object.values(world.objectOfId).find(object => object.info.Material)
        assert.ok(material)
        await input(container.querySelector('input[aria-label="Search by name or id"]'), String(material.id))
        await click(container.querySelector(`[data-object-id="${material.id}"]`))
        const alpha = [...container.querySelectorAll('label')].find(label => label.textContent === 'Alpha Mode')?.control
        assert.ok(alpha instanceof dom.window.HTMLButtonElement)
        assert.equal(alpha.getAttribute('role'), 'combobox')
        // Radix supports typeahead on a closed trigger without requiring layout APIs from jsdom.
        await key(alpha, 'm')
        let info = latest.world.objectOfId[material.id].info.Material
        assert.ok(info && typeof info === 'object' && !Array.isArray(info))
        assert.deepEqual(info.alphaMode, { Mask: { cutoff: 0.5 } })
        assert.ok(container.querySelector('input[aria-label="Cutoff"]'))
        // Clear the typeahead search before choosing the next variant.
        await new Promise(resolve => setTimeout(resolve, 1100))
        await key(alpha, 'b')
        info = latest.world.objectOfId[material.id].info.Material
        assert.ok(info && typeof info === 'object' && !Array.isArray(info))
        assert.equal(info.alphaMode, 'Blend')
        assert.equal(container.querySelector('input[aria-label="Cutoff"]'), null)
    })
})
