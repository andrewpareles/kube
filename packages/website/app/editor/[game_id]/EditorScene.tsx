'use client'

import Image from 'next/image'
import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { LoaderCircle } from 'lucide-react'
import { toast } from 'sonner'
import { LoadingKubeMark } from '@/components/site/LoadingKubeMark'
import type { BrowserWgpuRenderer } from '@/lib/gen/render_web/render_web'
import { createWgpuRenderResources } from '@/lib/rendering/createWgpuRenderResources'
import { urlOfPublicFile } from '@/lib/s3StorageFiles'
import { beginEditorPointer, moveEditorPointer, finishEditorPointer, type EditorPointer } from '../editorPointer'
import { getEditorCameraTarget, moveEditorCamera, type EditorCamera } from '../editorCamera'

// Show the same thumbnail and loading mark during server startup and GPU loading.
// TODO: AI-PICKED-VALUE: Cap the thumbnail at 576px.
export const EditorThumbnail = ({ thumbnailUuid, isLoading }: { thumbnailUuid: string | null, isLoading: boolean }) => <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
    <div className="relative aspect-video w-3/4 max-w-xl overflow-hidden rounded-xl shadow-sm">
        {thumbnailUuid !== null && <Image src={urlOfPublicFile({ kind: 'GameThumbnail', thumbnailUuid })} alt="Game thumbnail" fill sizes="576px" loading="eager" unoptimized className="object-contain" onError={event => { event.currentTarget.style.visibility = 'hidden' }} />}
        <div aria-hidden="true" className="absolute inset-0 bg-white/35" />
        {isLoading && <div role="status" aria-label="Loading editor" className="absolute inset-0 flex flex-col items-center justify-center gap-4 px-6">
            <LoadingKubeMark sidePx={80} renderId="editor-loading" />
            <p className="text-balance rounded-lg bg-white/80 px-3 py-1 text-center text-sm font-medium text-gray-700 shadow-sm backdrop-blur-sm">This will only take a second.</p>
        </div>}
    </div>
</div>

// TODO: AI-PICKED-VALUE: The time estimate under Building… is at most 320px wide, so it balances into two short lines.
const FirstDraftNotice = ({ status, hasPlacedObject }: { status: ReactNode, hasPlacedObject: boolean }) => hasPlacedObject
    ? <div className="pointer-events-none absolute inset-x-0 top-0 flex justify-center p-3">
        <div role="status" className="flex flex-col items-center gap-1 rounded-xl bg-white/90 px-5 py-3 text-center shadow-lg backdrop-blur-sm">
            <p className="flex items-center gap-2 text-lg font-semibold tracking-tight text-gray-950"><LoaderCircle className="size-5 shrink-0 animate-spin text-gray-400" /><span className="[&>span]:text-sm [&>span]:font-normal [&>span]:text-gray-500">{status}</span></p>
            <p className="max-w-xs text-balance text-xs text-gray-400">This usually takes 10-20 minutes, but can take longer for bigger builds. You can always leave and come back.</p>
        </div>
    </div>
    : <div className="pointer-events-none absolute inset-0 flex items-center justify-center p-6">
        <div role="status" className="flex max-w-md flex-col items-center gap-2 rounded-2xl bg-white/90 px-10 py-7 text-center shadow-lg backdrop-blur-sm">
            <p className="flex items-center gap-3 text-2xl font-semibold tracking-tight text-gray-950"><LoaderCircle className="size-6 shrink-0 animate-spin text-gray-400" />Building a first draft</p>
            <p className="text-sm text-gray-500">{status}</p>
        </div>
    </div>

// TODO: AI-PICKED-VALUE: Use the asset previews' initial framing before switching to the editor's free camera.
const initialView = { yaw: 0.55, pitch: 0.6, zoom: 1 }
// TODO: AI-PICKED-VALUE: Move 20 world units per second, capped at 100 ms per frame to avoid jumps after a pause.
const cameraSpeed = 20
// Match the desktop camera's mouse rotation, pixel scroll, wheel-line, and pinch sensitivities.
const cameraTouchRotateSensitivity = 0.003
const cameraScrollSensitivity = 0.006
const cameraWheelStep = 2
const cameraPinchSensitivity = 20
// TODO: AI-PICKED-VALUE: Double mouse-look sensitivity compared with the original desktop camera.
const cameraMouseRotateSensitivity = 0.006
// The arrow keys move the camera the same way as WASD.
const movementCodeOfArrowCode: { [code: string]: string | undefined } = { ArrowUp: 'KeyW', ArrowLeft: 'KeyA', ArrowDown: 'KeyS', ArrowRight: 'KeyD' }
export const EditorScene = ({ world, frameRequest, selectedObjectIds, onSelect, thumbnailUuid, cameraRef, firstDraftStatus, hasPlacedFirstDraftObject }: { world: string, frameRequest: { objectId: number } | null, selectedObjectIds: number[], onSelect: (objectId: number | null, isMultiSelect: boolean) => void, thumbnailUuid: string | null, cameraRef: RefObject<EditorCamera | null>, firstDraftStatus: ReactNode, hasPlacedFirstDraftObject: boolean }) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const resourcesRef = useRef<Promise<Awaited<ReturnType<typeof createWgpuRenderResources>>> | null>(null)
    const worldRef = useRef(world)
    useEffect(() => { worldRef.current = world }, [world])
    // Selection comes from React without restarting the scene or reloading its assets.
    const selectedObjectIdsRef = useRef(new Uint32Array(selectedObjectIds))
    useEffect(() => { selectedObjectIdsRef.current = new Uint32Array(selectedObjectIds) }, [selectedObjectIds])
    const frameRequestRef = useRef(frameRequest)
    useEffect(() => { frameRequestRef.current = frameRequest }, [frameRequest])
    const keysRef = useRef(new Set<string>())
    const isShiftPressedRef = useRef(false)
    const pointerOfIdRef = useRef<{ [id: number]: EditorPointer }>({})
    const pickObjectRef = useRef<((x: number, y: number) => number | null) | null>(null)
    const [didFail, setDidFail] = useState(false)
    const [didRender, setDidRender] = useState(false)
    useEffect(() => {
        const canvas = canvasRef.current
        if (!canvas) return
        const keyDown = (event: KeyboardEvent) => {
            // Read the modifier on movement keys too, in case Shift was held before the canvas gained focus.
            isShiftPressedRef.current = event.shiftKey
            const code = movementCodeOfArrowCode[event.code] ?? event.code
            if (event.metaKey || event.ctrlKey || event.altKey || !['KeyW', 'KeyA', 'KeyS', 'KeyD', 'KeyQ', 'KeyE'].includes(code)) return
            event.preventDefault()
            keysRef.current.add(code)
        }
        const keyUp = (event: KeyboardEvent) => { keysRef.current.delete(movementCodeOfArrowCode[event.code] ?? event.code); isShiftPressedRef.current = event.shiftKey }
        const clearInput = () => { keysRef.current.clear(); isShiftPressedRef.current = false; pointerOfIdRef.current = {} }
        const wheel = (event: WheelEvent) => {
            event.preventDefault()
            if (!cameraRef.current) return
            for (const pointer of Object.values(pointerOfIdRef.current)) pointer.isClick = false
            // Trackpad pinches arrive as ctrl+wheel; consume them so the browser does not zoom the page.
            const step = event.ctrlKey ? cameraPinchSensitivity / 100 : event.deltaMode === WheelEvent.DOM_DELTA_LINE ? cameraWheelStep : cameraScrollSensitivity * (event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? canvas.clientHeight : 1)
            moveEditorCamera(cameraRef.current, 1, 0, 0, -event.deltaY * step)
        }
        canvas.addEventListener('keydown', keyDown)
        canvas.addEventListener('blur', clearInput)
        canvas.addEventListener('wheel', wheel, { passive: false })
        window.addEventListener('keyup', keyUp)
        window.addEventListener('blur', clearInput)
        document.addEventListener('visibilitychange', clearInput)
        return () => {
            clearInput()
            canvas.removeEventListener('keydown', keyDown)
            canvas.removeEventListener('blur', clearInput)
            canvas.removeEventListener('wheel', wheel)
            window.removeEventListener('keyup', keyUp)
            window.removeEventListener('blur', clearInput)
            document.removeEventListener('visibilitychange', clearInput)
        }
    }, [cameraRef])
    useEffect(() => {
        const controller = new AbortController()
        const resourcesPromise = createWgpuRenderResources(controller.signal)
        resourcesRef.current = resourcesPromise
        return () => { controller.abort(); resourcesRef.current = null; void resourcesPromise.then(value => value.browserWgpuRenderContext.free()).catch(() => undefined) }
    }, [])
    useEffect(() => {
        const canvas = canvasRef.current
        const context = canvas?.getContext('2d', { alpha: false })
        const resourcesPromise = resourcesRef.current
        if (!canvas || !context || !resourcesPromise) return
        let isDisposed = false
        let renderer: BrowserWgpuRenderer | undefined
        let frame = 0
        let renderedWorld: string | undefined
        let previousFrameTime = performance.now()
        let loadingAssetCount = 0
        let finishedAssetLoadCount = 0
        let triedFrame: { request: { objectId: number }, finishedAssetLoadCount: number } | null = null
        const setup = async () => {
            const resources = await resourcesPromise
            if (isDisposed) return
            renderedWorld = worldRef.current
            renderer = resources.renderWeb.BrowserWgpuRenderer.from_world_json(resources.browserWgpuRenderContext, renderedWorld)
            // The renderer already handles absent assets and picks up successful uploads on subsequent frames.
            const loadAssets = (loadingRenderer: BrowserWgpuRenderer) => {
                loadingAssetCount++
                void resources.loadAssetsIntoGpu(loadingRenderer.mesh_data_ids(), loadingRenderer.texture_data_ids()).finally(() => { loadingAssetCount--; finishedAssetLoadCount++ })
            }
            loadAssets(renderer)
            setDidFail(false)
            const render = (time: number) => {
                if (isDisposed) return
                if (!renderer) return
                try {
                    if (renderedWorld !== worldRef.current) {
                        renderer.set_world(worldRef.current)
                        renderedWorld = worldRef.current
                        loadAssets(renderer)
                    }
                    const width = Math.max(1, Math.round(canvas.clientWidth * Math.min(devicePixelRatio, 2)))
                    const height = Math.max(1, Math.round(canvas.clientHeight * Math.min(devicePixelRatio, 2)))
                    if (canvas.width !== width) canvas.width = width
                    if (canvas.height !== height) canvas.height = height
                    if (resources.gpuCanvas.width !== width) resources.gpuCanvas.width = width
                    if (resources.gpuCanvas.height !== height) resources.gpuCanvas.height = height
                    if (cameraRef.current) {
                        const keys = keysRef.current
                        const speed = cameraSpeed / (isShiftPressedRef.current ? 5 : 1)
                        moveEditorCamera(cameraRef.current, Number(keys.has('KeyW')) - Number(keys.has('KeyS')), Number(keys.has('KeyD')) - Number(keys.has('KeyA')), Number(keys.has('KeyQ')) - Number(keys.has('KeyE')), speed * Math.min((time - previousFrameTime) / 1000, 0.1))
                        renderer.render_from_camera(resources.browserWgpuRenderContext, width, height, new Float32Array(cameraRef.current.position), new Float32Array(getEditorCameraTarget(cameraRef.current)), selectedObjectIdsRef.current)
                    } else {
                        renderer.render(resources.browserWgpuRenderContext, width, height, 0, initialView.yaw, initialView.pitch, initialView.zoom, true, false)
                        const [x, y, z] = renderer.camera_position()
                        cameraRef.current = { position: [x, y, z], yaw: initialView.yaw, pitch: initialView.pitch }
                    }
                    // Explorer double-clicks frame once, even if the same row is activated again. Viewport picking does not move the camera.
                    // A model has no size until its meshes download, so a request that finds nothing while they do tries again after.
                    const request = frameRequestRef.current
                    if (request && cameraRef.current && (triedFrame?.request !== request || triedFrame.finishedAssetLoadCount !== finishedAssetLoadCount)) {
                        const camera = cameraRef.current
                        const position = renderer.camera_position_for_object(resources.browserWgpuRenderContext, request.objectId, width / height, camera.yaw, camera.pitch)
                        if (position) { const [x, y, z] = position; camera.position = [x, y, z] }
                        if (position || loadingAssetCount === 0) frameRequestRef.current = null
                        else triedFrame = { request, finishedAssetLoadCount }
                    }
                    previousFrameTime = time
                    context.drawImage(resources.gpuCanvas, 0, 0)
                    pickObjectRef.current = (x, y) => renderer?.pick_object(resources.browserWgpuRenderContext, x, y) ?? null
                    setDidRender(true)
                    frame = requestAnimationFrame(render)
                } catch (error) { pickObjectRef.current = null; setDidFail(true); toast.error(String(error), { id: 'editor-render-error' }) }
            }
            render(performance.now())
        }
        void setup().catch(error => { if (!isDisposed) { setDidFail(true); toast.error(String(error), { id: 'editor-render-error' }) } })
        return () => { isDisposed = true; pickObjectRef.current = null; cancelAnimationFrame(frame); renderer?.free() }
    }, [cameraRef])
    return <div aria-busy={!didRender && !didFail} className="editor-loading-sky relative min-h-0 min-w-0 flex-1">
        {/* The opaque canvas starts black; reveal it only after copying the first rendered frame. */}
        <canvas ref={canvasRef} aria-label="Game world" tabIndex={0} className={`absolute inset-0 size-full touch-none cursor-grab outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-gray-400 active:cursor-grabbing ${didRender ? '' : 'invisible'}`} onContextMenu={event => event.preventDefault()} onPointerDown={event => {
            if (event.button !== 0 && event.button !== 2) return
            event.currentTarget.focus({ preventScroll: true })
            // Capture keeps the drag active outside the viewport without hiding or locking the cursor.
            event.currentTarget.setPointerCapture(event.pointerId)
            beginEditorPointer(pointerOfIdRef.current, event.pointerId, event.clientX, event.clientY, event.button)
        }} onPointerMove={event => {
            const camera = cameraRef.current
            const pointer = pointerOfIdRef.current[event.pointerId]
            if (!camera || !pointer) return
            const points = Object.values(pointerOfIdRef.current)
            const previousDistance = points.length === 2 ? Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y) : 0
            if (event.pointerType === 'mouse' && event.buttons !== 1) pointer.isClick = false
            const { dx, dy } = moveEditorPointer(pointer, event.clientX, event.clientY)
            if (points.length === 2) {
                const distance = Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y)
                if (previousDistance > 0 && distance > 0) moveEditorCamera(camera, 1, 0, 0, Math.log(distance / previousDistance) * cameraPinchSensitivity)
            } else if (points.length === 1) {
                const sensitivity = event.pointerType === 'mouse' ? cameraMouseRotateSensitivity : cameraTouchRotateSensitivity
                camera.yaw -= dx * sensitivity
                camera.pitch = Math.max(-1.45, Math.min(1.45, camera.pitch + dy * sensitivity))
            }
        }} onPointerUp={event => {
            const click = finishEditorPointer(pointerOfIdRef.current, event.pointerId, event.clientX, event.clientY)
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
            const pick = pickObjectRef.current
            if (!click || !pick) return
            const bounds = event.currentTarget.getBoundingClientRect()
            const x = (click.x - bounds.left) / bounds.width, y = (click.y - bounds.top) / bounds.height
            if (x < 0 || x > 1 || y < 0 || y > 1 || !Number.isFinite(x) || !Number.isFinite(y)) return
            // CSS coordinates normalize independently of the canvas's device-pixel resolution.
            try { onSelect(pick(x * 2 - 1, 1 - y * 2), event.shiftKey || event.metaKey || event.ctrlKey) }
            catch (error) { toast.error(String(error), { id: 'editor-pick-error' }) }
        }} onPointerCancel={event => { delete pointerOfIdRef.current[event.pointerId] }} onLostPointerCapture={event => { delete pointerOfIdRef.current[event.pointerId] }} />
        {!didRender && <EditorThumbnail thumbnailUuid={thumbnailUuid} isLoading />}
        {didRender && firstDraftStatus !== null && <FirstDraftNotice status={firstDraftStatus} hasPlacedObject={hasPlacedFirstDraftObject} />}
    </div>
}
