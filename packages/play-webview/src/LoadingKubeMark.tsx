import { useEffect, useRef, useState } from 'react'
import { advanceWanderState, createWanderState, fastWanderSpeed, getRestingFrame, renderKubeLogoSvg, restingKView, tumblingKHoverEffect } from '../../website/lib/kubeLogo/kubeLogoRender'

// Run the existing 3D K tumble continuously while loading, instead of waiting for hover.
export const LoadingKubeMark = ({ sidePx, renderId }: { sidePx: number, renderId: string }) => {
    const markRef = useRef<HTMLSpanElement>(null)
    // Keep the initial SVG stable so a parent render does not reset the animation.
    const [initialMarkup] = useState(() => ({
        __html: renderKubeLogoSvg({ frame: getRestingFrame(restingKView), renderId, sidePx, strokeScale: 1.25 }),
    }))

    useEffect(() => {
        const mark = markRef.current
        if (!mark || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
        const wanderState = createWanderState()
        let previousTimeMs = performance.now()
        let frameId = 0
        const step = (currentTimeMs: number) => {
            advanceWanderState({
                wanderState,
                currentTimeMs,
                elapsedSeconds: Math.min(64, currentTimeMs - previousTimeMs) / 1000,
                speedRange: fastWanderSpeed,
                isGoingHome: false,
            })
            previousTimeMs = currentTimeMs
            mark.innerHTML = renderKubeLogoSvg({
                frame: { ...tumblingKHoverEffect.frameAt(1, { baseView: restingKView, wanderState }), hiddenEdgeOpacity: 0 },
                renderId,
                sidePx,
                strokeScale: 1.25,
            })
            frameId = window.requestAnimationFrame(step)
        }
        frameId = window.requestAnimationFrame(step)
        return () => window.cancelAnimationFrame(frameId)
    }, [renderId, sidePx])

    return <span ref={markRef} aria-hidden="true" className="block" dangerouslySetInnerHTML={initialMarkup} />
}

export const RestingKubeMark = ({ sidePx, renderId }: { sidePx: number, renderId: string }) => (
    <span aria-hidden="true" className="block" dangerouslySetInnerHTML={{ __html: renderKubeLogoSvg({ frame: getRestingFrame(restingKView), renderId, sidePx, strokeScale: 1.25 }) }} />
)
