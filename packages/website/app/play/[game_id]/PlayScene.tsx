'use client'

import { useEffect, useRef, useState } from 'react'
import { LoadingKubeMark } from '@/components/site/LoadingKubeMark'
import type { BrowserPlaySession } from '@/lib/gen/play_web/play_web'

// TODO: AI-PICKED-VALUE: Four readouts a second is enough to watch the frame time and step count, and cheaper than reading
// them every frame.
const debugReadoutIntervalMs = 250
// Holding a movement key must not scroll the page or reach the browser's own shortcuts.
const movementCodes = ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'ArrowUp', 'ArrowLeft', 'ArrowDown', 'ArrowRight']
// The site answers with the game's address as soon as its server is running, and a starting server can take a while.
const connectionInfoPollIntervalMs = 1000

type WebtransportInfo = {
    host: string
    port: number
    certificate_sha256_base64: string | null
}

// A page dials the server by name on the HTTP/3 port Edgegap published for it.
const readServerAddress = async ({ requestId, playAuth }: { requestId: string; playAuth: string }) => {
    while (true) {
        const response = await fetch(`/api/play-sessions/${requestId}/connection-info`, {
            headers: { authorization: `Bearer ${playAuth}` },
            cache: 'no-store',
        })
        if (response.status === 202) {
            await new Promise((resolve) => window.setTimeout(resolve, connectionInfoPollIntervalMs))
            continue
        }
        if (!response.ok) {
            const body: { error?: string } = await response.json().catch(() => ({}))
            throw new Error(body.error ?? 'This game could not be joined.')
        }
        const connectionInfo: { webtransport: WebtransportInfo | null } = await response.json()
        if (connectionInfo.webtransport === null) {
            throw new Error("This game's server is not serving browsers yet.")
        }
        const { host, port, certificate_sha256_base64 } = connectionInfo.webtransport
        const certificateHash =
            certificate_sha256_base64 === null
                ? undefined
                : Uint8Array.from(window.atob(certificate_sha256_base64), (character) => character.charCodeAt(0))
        return { serverUrl: `https://${host}:${port}/play`, certificateHash }
    }
}

// PlayScene joins a game the site started and draws it: the canvas fills the window, and the play session applies the
// server's world and steps it.
export const PlayScene = ({ requestId, playAuth }: { requestId: string; playAuth: string }) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const sessionRef = useRef<BrowserPlaySession | null>(null)
    const [failure, setFailure] = useState<string | null>(null)
    const [isPlaying, setIsPlaying] = useState(false)
    const [isPointerLocked, setIsPointerLocked] = useState(false)
    const [frameMs, setFrameMs] = useState(0)
    const [physicsStepCount, setPhysicsStepCount] = useState(0)
    const [pingMs, setPingMs] = useState(0)
    const [characterPosition, setCharacterPosition] = useState('')

    // The connection and its simulation live in the play session, which draws from the window's animation frames.
    useEffect(() => {
        const canvas = canvasRef.current
        if (!canvas) return
        let isDisposed = false
        let frameRequest = 0

        // The canvas keeps its own drawing size separate from the size it is laid out at, so it is drawn at the screen's
        // pixel density and resized with the window.
        const sizeCanvasToWindow = () => {
            canvas.width = Math.max(1, Math.round(window.innerWidth * window.devicePixelRatio))
            canvas.height = Math.max(1, Math.round(window.innerHeight * window.devicePixelRatio))
        }
        sizeCanvasToWindow()
        window.addEventListener('resize', sizeCanvasToWindow)

        const start = async () => {
            const { serverUrl, certificateHash } = await readServerAddress({ requestId, playAuth })
            const playWeb = await import('@/lib/gen/play_web/play_web')
            await playWeb.default()
            // Loading the module takes a moment, and a mount React threw away in the meantime should not join a game.
            if (isDisposed) return
            const session = await playWeb.BrowserPlaySession.connect(canvas, serverUrl, playAuth, certificateHash)
            if (isDisposed) {
                session.free()
                return
            }
            sessionRef.current = session
            setIsPlaying(true)
            const drawFrame = (nowMs: number) => {
                // A frame the browser queued before this effect was cleaned up must not reach the freed session.
                if (isDisposed) return
                try {
                    session.render_frame(nowMs)
                } catch (error) {
                    setFailure(error instanceof Error ? error.message : String(error))
                    return
                }
                frameRequest = window.requestAnimationFrame(drawFrame)
            }
            frameRequest = window.requestAnimationFrame(drawFrame)
        }
        start().catch((error: unknown) => { if (!isDisposed) setFailure(error instanceof Error ? error.message : String(error)) })

        return () => {
            isDisposed = true
            window.cancelAnimationFrame(frameRequest)
            frameRequest = 0
            window.removeEventListener('resize', sizeCanvasToWindow)
            sessionRef.current?.free()
            sessionRef.current = null
            setIsPlaying(false)
        }
    }, [requestId, playAuth])

    // Keys go to the session by their browser code, so the same keys work on any keyboard layout.
    useEffect(() => {
        const sendKey = (event: KeyboardEvent, isDown: boolean) => {
            if (!movementCodes.includes(event.code)) return
            event.preventDefault()
            sessionRef.current?.set_key(event.code, isDown)
        }
        const onKeyDown = (event: KeyboardEvent) => sendKey(event, true)
        const onKeyUp = (event: KeyboardEvent) => sendKey(event, false)
        window.addEventListener('keydown', onKeyDown)
        window.addEventListener('keyup', onKeyUp)
        return () => {
            window.removeEventListener('keydown', onKeyDown)
            window.removeEventListener('keyup', onKeyUp)
        }
    }, [])

    // Clicking the canvas takes the pointer, and the movement it reports while it is taken turns the camera.
    useEffect(() => {
        const canvas = canvasRef.current
        if (!canvas) return
        const onPointerMove = (event: MouseEvent) => {
            if (document.pointerLockElement !== canvas) return
            sessionRef.current?.turn_camera(event.movementX, event.movementY)
        }
        const onPointerLockChange = () => setIsPointerLocked(document.pointerLockElement === canvas)
        document.addEventListener('mousemove', onPointerMove)
        document.addEventListener('pointerlockchange', onPointerLockChange)
        return () => {
            document.removeEventListener('mousemove', onPointerMove)
            document.removeEventListener('pointerlockchange', onPointerLockChange)
        }
    }, [])

    // The debug readout asks the session how the last frame went, rather than every frame.
    useEffect(() => {
        const interval = window.setInterval(() => {
            const session = sessionRef.current
            if (session === null) return
            setFrameMs(session.last_frame_ms())
            setPhysicsStepCount(session.num_physics_steps_taken())
            setPingMs(session.ping_ms())
            setCharacterPosition(Array.from(session.character_position(), axis => axis.toFixed(2)).join(', '))
        }, debugReadoutIntervalMs)
        return () => window.clearInterval(interval)
    }, [])

    return <div className="fixed inset-0 overflow-hidden bg-gray-950">
        <canvas ref={canvasRef} onClick={() => { canvasRef.current?.requestPointerLock().catch(() => {}) }} className="h-full w-full touch-none" />
        {!isPlaying && failure === null && <div role="status" aria-label="Joining the game" className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-3">
            <LoadingKubeMark sidePx={80} renderId="play-loading" />
            <p className="text-sm text-white/60">Joining the game…</p>
        </div>}
        {failure !== null && <div role="alert" className="absolute inset-0 flex items-center justify-center p-6">
            <p className="max-w-md text-balance rounded-xl bg-white/90 px-5 py-4 text-center text-sm text-gray-700 shadow-lg backdrop-blur-sm">{failure}</p>
        </div>}
        {isPlaying && <div aria-hidden="true" className="pointer-events-none absolute bottom-3 left-3 flex flex-col gap-0.5 rounded-lg bg-gray-950/60 px-2.5 py-1.5 font-mono text-xs text-white/80">
            <span>{frameMs.toFixed(1)} ms per frame</span>
            <span>{physicsStepCount} physics steps</span>
            <span>{pingMs.toFixed(0)} ms ping</span>
            {characterPosition !== '' && <span>at {characterPosition}</span>}
            {!isPointerLocked && <span className="text-white/50">Click to look around</span>}
        </div>}
    </div>
}
