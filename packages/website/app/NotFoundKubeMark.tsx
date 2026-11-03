'use client'

import { useEffect, useRef, useState } from 'react'
import {
    getRestingFrame,
    renderKubeLogoSvg,
    restingKView,
} from '@/lib/kubeLogo/kubeLogoRender'

/**
 * The K on the 404 page, growing its prism out from the middle of the cube and shrinking it back
 * to a slab again, over and over. It is the same drawing the header and the fold use, on the one
 * frame value that says how deep the K is cut, so nothing new about the shape is invented here.
 *
 * The slab sits around the middle of the cube rather than against its front face, so the K grows
 * and shrinks about its own centre instead of appearing to slide towards the reader.
 *
 * It runs on its own rather than on hover, because a page nobody meant to land on is not a page
 * anybody is going to think to point at.
 */

// TODO: AI-PICKED-VALUE: slow enough to read as breathing rather than flashing.
const extrusionCycleMs = 3200
const shallowExtrusionDepth = 0.6
const fullExtrusionDepth = 1

export const NotFoundKubeMark = ({ sidePx, renderId }: { sidePx: number; renderId: string }) => {
    const markRef = useRef<HTMLSpanElement>(null)

    // The first paint has to survive server rendering, so it goes into the markup as a string.
    // It is held as the whole prop object because React only leaves this element alone when it is
    // handed back the very same one, and a fresh object would wipe out whatever the loop drew.
    const [initialMarkup] = useState(() => ({
        __html: renderKubeLogoSvg({
            frame: { ...getRestingFrame(restingKView), extrusionDepth: shallowExtrusionDepth, extrusionAlignment: 'center' },
            renderId,
            sidePx,
            strokeScale: 1,
        }),
    }))

    useEffect(() => {
        const mark = markRef.current
        if (mark === null) return
        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return

        let frameId: number | null = null

        const step = (currentTimeMs: number) => {
            // A cosine is what makes it ease at both ends, so the K settles for a moment when it
            // is the full prism and when it is back to a slab, instead of bouncing between them.
            // It starts at the shallow end, which is the slab the first paint drew.
            const cycleProgress = (1 - Math.cos(currentTimeMs / extrusionCycleMs * 2 * Math.PI)) / 2
            const extrusionDepth = shallowExtrusionDepth
                + (fullExtrusionDepth - shallowExtrusionDepth) * cycleProgress

            mark.innerHTML = renderKubeLogoSvg({
                frame: { ...getRestingFrame(restingKView), extrusionDepth, extrusionAlignment: 'center' },
                renderId,
                sidePx,
                strokeScale: 1,
            })
            frameId = window.requestAnimationFrame(step)
        }

        frameId = window.requestAnimationFrame(step)

        return () => {
            if (frameId !== null) window.cancelAnimationFrame(frameId)
        }
    }, [renderId, sidePx])

    return <span ref={markRef} className="block" dangerouslySetInnerHTML={initialMarkup} />
}
