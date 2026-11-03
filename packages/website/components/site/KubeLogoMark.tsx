'use client'

import { useEffect, useRef, useState } from 'react'
import {
    advanceWanderState,
    createWanderState,
    getEasedProgress,
    getIsWanderSettled,
    getLevelLegsFrame,
    getRestingMarkBounds,
    logoMorphMilliseconds,
    renderKubeLogoSvg,
    restingLogoPreset,
    type LogoHoverEffect,
    type LogoView,
} from '@/lib/kubeLogo/kubeLogoRender'

/**
 * The cube-K mark. It draws itself as SVG on every animation frame rather than transitioning
 * CSS, because the shape itself changes: the cuts close, the camera turns, and the colours
 * cross-fade together.
 *
 * Hovering plays the next effect in `hoverCycle` and holds it; leaving plays it back to rest. On a
 * screen with no pointer, a tap is the hover and it holds until the next tap lands elsewhere.
 *
 * Pass `label` as null where a wordmark next to the mark already names it, so screen readers do
 * not read the name twice.
 *
 * `introMilliseconds` plays the first cycle entry backwards once when the page loads, so the
 * fold's mark arrives as a closed cube and opens into the K. Pass null to start at rest.
 *
 * `sidePx` is the size the drawing is generated at, not the size this takes up. The K fills only
 * the middle of the square the generator works in, so the element is only as big as the K itself
 * and the drawing hangs out of it. That way the pointer answers where the mark looks like it is,
 * and a link wrapped around it is clickable over exactly the same region.
 *
 * `isKLevel` turns the K a fraction of a degree so the bottom of its legs runs flat across the
 * page. It is worth it small, where a leg that drops a pixel across its length just looks badly
 * drawn, and not worth it large, where the K reads as sitting on the cube it is cut from.
 *
 * `hoverAreaOverhangPx` is how far past the K the pointer is still answered. It is invisible and
 * takes up no room, so the mark still measures and sits exactly where it did; it only makes the
 * mark easier to land on than its own outline. It is a number of pixels rather than a share of
 * the drawing because the small mark in the header and the big one on the front page want
 * different amounts, and not in proportion to their sizes.
 */
export const KubeLogoMark = ({ view, hoverCycle, sidePx, hoverAreaOverhangPx, strokeScale, renderId, label, introMilliseconds, isKLevel }: {
    view: LogoView
    hoverCycle: LogoHoverEffect[]
    sidePx: number
    hoverAreaOverhangPx: number
    strokeScale: number
    renderId: string
    label: string | null
    introMilliseconds: number | null
    isKLevel: boolean
}) => {
    const hoverAreaRef = useRef<HTMLSpanElement>(null)
    const markRef = useRef<HTMLSpanElement>(null)
    const markBounds = getRestingMarkBounds({ view, strokeScale, isKLevel, preset: restingLogoPreset })

    // The first paint has to survive server rendering, so it is a plain string in the markup. It
    // is the cycle's own frame at whichever end the animation starts from, rather than the
    // generic resting drawing: that one is always the opened K, so a cycle that rests as the
    // closed cube could only reach it by playing an intro.
    //
    // It is held as the whole prop object rather than as the string inside it, because React
    // only leaves this element alone when it is handed back the very same object. Given a fresh
    // one it writes the markup again, which would wipe out whatever the animation had drawn and
    // snap the mark back to how it started every time the page around it re-rendered.
    const [initialMarkup] = useState(() => {
        const startProgress = introMilliseconds === null ? 0 : 1
        const startFrame = hoverCycle[introMilliseconds === null ? hoverCycle.length - 1 : 0]
            .frameAt(startProgress, { baseView: view, wanderState: createWanderState() })

        return {
            __html: renderKubeLogoSvg({
                // The same levelling paint() applies at this progress, so the handover to the
                // animation loop does not shift the drawing.
                frame: isKLevel ? getLevelLegsFrame(startFrame, 1 - getEasedProgress(startProgress)) : startFrame,
                renderId,
                sidePx,
                strokeScale,
            }),
        }
    })

    // All animation state lives in one ref: React state would re-render on every frame.
    const animationRef = useRef({
        progress: introMilliseconds === null ? 0 : 1,
        targetProgress: 0,
        activeEffectIndex: introMilliseconds === null ? hoverCycle.length - 1 : 0,
        isPlayingIntro: introMilliseconds !== null,
        lastFrameTimeMs: null as number | null,
        frameId: null as number | null,
        wanderState: createWanderState(),
    })

    useEffect(() => {
        const mark = markRef.current
        // A brand link owns hover and keyboard focus for both its name and its mark.
        const hoverArea = hoverAreaRef.current?.closest<HTMLElement>('[data-kube-logo-hover]') ?? hoverAreaRef.current
        if (mark === null || hoverArea === null) return

        const animation = animationRef.current
        const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches

        const paint = () => {
            const effect = hoverCycle[animation.activeEffectIndex]
            const easedProgress = getEasedProgress(animation.progress)
            const frame = effect.frameAt(easedProgress, {
                baseView: view,
                wanderState: animation.wanderState,
            })
            // The levelling is strongest at rest and gone once a hover has played, so what the
            // mark does when it moves is the same either way.
            const paintedFrame = isKLevel ? getLevelLegsFrame(frame, 1 - easedProgress) : frame
            mark.innerHTML = renderKubeLogoSvg({ frame: paintedFrame, renderId, sidePx, strokeScale })
        }

        const step = (currentTimeMs: number) => {
            const elapsedMs = animation.lastFrameTimeMs === null
                ? 0
                : Math.min(64, currentTimeMs - animation.lastFrameTimeMs)
            animation.lastFrameTimeMs = currentTimeMs

            // The opening move runs at its own slower pace; everything after it is a hover.
            const durationMs = animation.isPlayingIntro && introMilliseconds !== null
                ? introMilliseconds
                : logoMorphMilliseconds
            const direction = animation.targetProgress > animation.progress ? 1 : -1
            animation.progress = Math.min(1, Math.max(0, animation.progress + (elapsedMs / durationMs) * direction))
            if (animation.progress === 0) animation.isPlayingIntro = false

            // A tumbling effect keeps turning for as long as the pointer stays on it, and turns
            // itself back to square once the pointer has gone. Both are the drift's own doing, so
            // catching it half way home and hovering again simply carries on from the angle it is
            // at rather than picking up an arc worked out from how far through the hover it is.
            const effect = hoverCycle[animation.activeEffectIndex]
            if (effect.wanderSpeedRange !== null) {
                advanceWanderState({
                    wanderState: animation.wanderState,
                    currentTimeMs,
                    elapsedSeconds: elapsedMs / 1000,
                    speedRange: effect.wanderSpeedRange,
                    isGoingHome: animation.targetProgress === 0,
                })
            }
            paint()

            // A tumble has to keep being drawn for as long as the pointer is on it, and then for
            // as long as it takes to get home, which can outlast the hover unwinding. Only asking
            // whether it had got home would stop the loop the moment the hover had played in,
            // because a mark that is being hovered is not on its way anywhere.
            const isStillTurning = effect.wanderSpeedRange !== null
                && (animation.targetProgress === 1 || !getIsWanderSettled(animation.wanderState))
            const shouldKeepAnimating = animation.progress !== animation.targetProgress || isStillTurning
            if (shouldKeepAnimating) {
                animation.frameId = window.requestAnimationFrame(step)
                return
            }
            animation.frameId = null
            animation.lastFrameTimeMs = null
        }

        const moveTo = (nextTargetProgress: number) => {
            animation.targetProgress = nextTargetProgress
            if (prefersReducedMotion) {
                animation.progress = nextTargetProgress
                paint()
                return
            }
            if (animation.frameId === null) {
                animation.lastFrameTimeMs = null
                animation.frameId = window.requestAnimationFrame(step)
            }
        }

        const beginHover = (event: PointerEvent) => {
            // A tap or focused link must not leave the header folded into a cube.
            if (event.pointerType === 'touch') return
            // Touching the mark hands control to the pointer, so the opening move stops being special.
            animation.isPlayingIntro = false
            // Starting from rest begins a fresh move: step the cycle on, and clear any turn and
            // speed left over from the last hover so a tumble always sets off from where the mark
            // is actually sitting rather than snapping to where it stopped last time.
            if (animation.progress === 0) {
                animation.activeEffectIndex = (animation.activeEffectIndex + 1) % hoverCycle.length
                animation.wanderState = createWanderState()
            }
            moveTo(1)
        }

        // A finger lifting is the end of a tap, not the end of paying attention to the mark, so a
        // touch holds what it played instead of unwinding straight away. Without this a tap plays
        // the move in and reverses it in the same breath, which reads as a flicker.
        // Hover alone now controls the fold; the earlier touch behavior described above is retired.
        const endHover = () => {
            moveTo(0)
        }

        // What ends it instead is the next touch landing somewhere else, which is as close as a
        // screen with no pointer gets to moving away from something.

        // The mark is its own hover target, so it animates whether or not it sits inside a link.
        hoverArea.addEventListener('pointerenter', beginHover)
        hoverArea.addEventListener('pointerleave', endHover)

        // Drawing once here is what makes the levelling switch take effect, since a resting mark
        // has no animation frame running to pick the change up.
        paint()

        // Run the opening move as soon as the mark is on the page.
        if (animation.isPlayingIntro) moveTo(0)

        return () => {
            hoverArea.removeEventListener('pointerenter', beginHover)
            hoverArea.removeEventListener('pointerleave', endHover)
            // Clearing the frame handle matters as much as cancelling it. The animation state
            // outlives this effect, so leaving a stale handle behind would make the next run
            // believe a loop was already going and quietly skip starting one.
            if (animation.frameId !== null) window.cancelAnimationFrame(animation.frameId)
            animation.frameId = null
            animation.lastFrameTimeMs = null
        }
    }, [view, hoverCycle, sidePx, strokeScale, renderId, introMilliseconds, isKLevel])

    // The outer span is the mark as far as the rest of the page is concerned: it is the size of
    // the drawing, it is what the pointer answers to, and it is what a link around it wraps.
    //
    // The reach span is what widens that. It is empty and see-through, and being positioned it
    // takes up no room, so the mark still measures and sits exactly where it did. Hovering it
    // counts as hovering the outer span, and a press on it reaches any link around the mark.
    //
    // The inner one is the full square the generator draws into, hung off the outer one so the
    // drawing lands where it should. Nothing inside it can be touched, for two reasons: it spills
    // well outside the outer span, which would put the empty corners of the square back in play,
    // and the drawing is rebuilt every animation frame, so a press could otherwise land on a
    // shape that is gone by the time the button comes back up, losing the click.
    return (
        <span
            ref={hoverAreaRef}
            aria-label={label ?? undefined}
            aria-hidden={label === null}
            role={label === null ? undefined : "img"}
            className="relative inline-block leading-none"
            style={{ width: sidePx * markBounds.width, height: sidePx * markBounds.height }}
        >
            <span className="absolute" style={{ inset: -hoverAreaOverhangPx }} />
            <span
                ref={markRef}
                className="pointer-events-none absolute"
                style={{
                    left: -sidePx * markBounds.left,
                    top: -sidePx * markBounds.top,
                    width: sidePx,
                    height: sidePx,
                }}
                dangerouslySetInnerHTML={initialMarkup}
            />
        </span>
    )
}
