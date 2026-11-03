'use client'

import { useEffect, useState, type PointerEvent } from 'react'
import { useRouter } from 'next/navigation'
import { toast } from 'sonner'
import posthog from 'posthog-js'
import { saveAvatarLook } from '@/app/server-actions/avatar'
import { GUEST_COLOR, useAvatarDraft, type AvatarDraft } from '@/components/site/AvatarDraft'
import { CatalogItemCardBody, type CatalogCardItem } from '@/components/site/CatalogItemCard'
import { NoHatsYet } from '@/components/site/NoHatsYet'
import type { PlayerLook } from '@/lib/siteAvatar'
import { Button } from '@/shadcn/components/ui/button'

// TODO: AI-PICKED-VALUE: Twelve common colors to pick with one click.
const QUICK_COLORS = [
    { name: 'Red', color: '#e53935' },
    { name: 'Orange', color: '#fb8c00' },
    { name: 'Yellow', color: '#fdd835' },
    { name: 'Green', color: '#43a047' },
    { name: 'Teal', color: '#00897b' },
    { name: 'Blue', color: GUEST_COLOR },
    { name: 'Navy', color: '#283593' },
    { name: 'Pink', color: '#ec407a' },
    { name: 'Brown', color: '#795548' },
    { name: 'Gray', color: '#9e9e9e' },
    { name: 'White', color: '#ffffff' },
    { name: 'Black', color: '#212121' },
]

type Hsv = { h: number, s: number, v: number }

const hexToHsv = (hex: string): Hsv => {
    const [r, g, b] = [1, 3, 5].map(start => parseInt(hex.slice(start, start + 2), 16) / 255)
    const max = Math.max(r, g, b)
    const range = max - Math.min(r, g, b)
    const sixths = range === 0 ? 0 : max === r ? ((g - b) / range + 6) % 6 : max === g ? (b - r) / range + 2 : (r - g) / range + 4
    return { h: sixths * 60, s: max === 0 ? 0 : range / max, v: max }
}

const hsvToHex = ({ h, s, v }: Hsv) => {
    const channel = (n: number) => {
        const k = (n + h / 60) % 6
        return Math.round((v - v * s * Math.max(0, Math.min(k, 4 - k, 1))) * 255).toString(16).padStart(2, '0')
    }
    return `#${channel(5)}${channel(3)}${channel(1)}`
}

const pointerFraction = (event: PointerEvent<HTMLElement>, axis: 'x' | 'y') => {
    const rect = event.currentTarget.getBoundingClientRect()
    const fraction = axis === 'x' ? (event.clientX - rect.left) / rect.width : (event.clientY - rect.top) / rect.height
    return Math.min(1, Math.max(0, fraction))
}

// A drag keeps picking until the pointer lifts, even when it leaves the field.
const dragHandlers = (pick: (event: PointerEvent<HTMLDivElement>) => void) => ({
    onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
        event.currentTarget.setPointerCapture(event.pointerId)
        pick(event)
    },
    onPointerMove: (event: PointerEvent<HTMLDivElement>) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) pick(event)
    },
})

const handleClassName = 'pointer-events-none absolute size-5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow ring-1 ring-black/20'

// A saturation and brightness square over a hue bar. The field keeps its own hue, which a gray or black color would forget.
const ColorField = ({ color, onChange }: { color: string, onChange: (color: string) => void }) => {
    const [hsv, setHsv] = useState(() => hexToHsv(color))
    const [fieldColor, setFieldColor] = useState(color)
    if (color !== fieldColor) {
        setFieldColor(color)
        setHsv(hexToHsv(color))
    }
    const pick = (next: Hsv) => {
        const nextColor = hsvToHex(next)
        setHsv(next)
        setFieldColor(nextColor)
        onChange(nextColor)
    }

    return (
        <div className="w-full max-w-56 space-y-3">
            <div
                role="slider"
                aria-label="Saturation and brightness"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(hsv.s * 100)}
                aria-valuetext={color}
                className="relative aspect-[4/3] w-full cursor-crosshair touch-none rounded-xl"
                style={{ backgroundColor: `hsl(${hsv.h} 100% 50%)`, backgroundImage: 'linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent)' }}
                {...dragHandlers(event => pick({ h: hsv.h, s: pointerFraction(event, 'x'), v: 1 - pointerFraction(event, 'y') }))}
            >
                <span className={handleClassName} style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%`, backgroundColor: color }} />
            </div>
            <div
                role="slider"
                aria-label="Hue"
                aria-valuemin={0}
                aria-valuemax={360}
                aria-valuenow={Math.round(hsv.h)}
                className="relative h-3 w-full cursor-pointer touch-none rounded-full"
                style={{ backgroundImage: 'linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)' }}
                {...dragHandlers(event => pick({ ...hsv, h: pointerFraction(event, 'x') * 360 }))}
            >
                <span className={`${handleClassName} top-1/2`} style={{ left: `${(hsv.h / 360) * 100}%`, backgroundColor: `hsl(${hsv.h} 100% 50%)` }} />
            </div>
        </div>
    )
}

// Asks before a link or a back or forward step leaves the page, and calls onLeave when the player agrees. Reloading, closing the tab and
// links to other sites get the browser's own warning, whose words a page cannot set.
const warnBeforeLeaving = (onLeave: () => void) => {
    // TypeScript's DOM library does not yet include the Navigation API.
    type NavigateEvent = Event & { navigationType: string, destination: { sameDocument: boolean }, hashChange: boolean }
    const { navigation } = window as Window & { navigation?: EventTarget }
    let hasAgreedToLeave = false
    const confirmLeave = (event: Event) => {
        if (window.confirm('Are you sure you want to leave without saving?')) {
            hasAgreedToLeave = true
            onLeave()
        } else {
            event.preventDefault()
        }
    }
    const click = (event: MouseEvent) => {
        const link = event.target instanceof Element ? event.target.closest('a[href]') : null
        if (!(link instanceof HTMLAnchorElement) || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
        if ((link.target !== '' && link.target !== '_self') || link.hasAttribute('download')) return
        const url = new URL(link.href)
        if (url.origin === window.location.origin && (url.pathname !== window.location.pathname || url.search !== window.location.search)) confirmLeave(event)
    }
    const navigate = (event: Event) => {
        const { navigationType, destination, hashChange } = event as NavigateEvent
        if (navigationType === 'traverse' && destination.sameDocument && event.cancelable && !hashChange) confirmLeave(event)
    }
    const beforeUnload = (event: BeforeUnloadEvent) => {
        if (hasAgreedToLeave) return
        event.preventDefault()
        event.returnValue = ''
    }
    window.addEventListener('click', click, true)
    navigation?.addEventListener('navigate', navigate)
    window.addEventListener('beforeunload', beforeUnload)
    return () => {
        window.removeEventListener('click', click, true)
        navigation?.removeEventListener('navigate', navigate)
        window.removeEventListener('beforeunload', beforeUnload)
    }
}

const discardChanges = (setDraft: (draft: AvatarDraft) => void) => setDraft({ color: undefined, hat: undefined })

const useAvatarDraftOfEditor = () => {
    const avatarDraft = useAvatarDraft()
    if (avatarDraft === null) throw new Error('AvatarEditor must be inside an AvatarDraftProvider.')
    return avatarDraft
}

// My Avatar tries hats and colors on the avatar at the top of the profile. Clicking the hat being worn takes it off. While anything has
// changed, Save and Discard fade in at the bottom, keeping their space when hidden: Save makes the changes the look the player wears in
// games, and Discard goes back to the saved look. Leaving with changes unsaved asks first and drops them.
// Hat cards sit in the site's card grid, like every other card.
export const AvatarEditor = ({ look, hats }: { look: PlayerLook, hats: CatalogCardItem[] }) => {
    const router = useRouter()
    const { draft, setDraft } = useAvatarDraftOfEditor()
    const [isSaving, setIsSaving] = useState(false)
    const color = draft.color ?? look.preferredColor ?? GUEST_COLOR
    const savedHatId = look.equippedItem?.catalogItemId ?? null
    const hatId = draft.hat === undefined ? savedHatId : draft.hat?.catalogItemId ?? null
    const isChanged = (draft.color !== undefined && draft.color !== look.preferredColor) || hatId !== savedHatId

    useEffect(() => {
        if (!isChanged) return
        return warnBeforeLeaving(() => discardChanges(setDraft))
    }, [isChanged, setDraft])

    const change = (next: Partial<AvatarDraft>) => setDraft({ ...draft, ...next })

    const save = async () => {
        setIsSaving(true)
        try {
            await saveAvatarLook({ color: draft.color ?? look.preferredColor, catalogItemId: hatId })
            posthog.capture('site-avatar-save')
            router.refresh()
        } catch {
            toast.error('Could not save your avatar. Please try again.')
        } finally {
            setIsSaving(false)
        }
    }

    return (
        <div>
            <div className="space-y-16">
                <section>
                    <h2 className="mb-4 text-lg font-semibold tracking-tight">Hats</h2>
                    {hats.length === 0 ? <NoHatsYet /> : (
                        <div className="site-game-grid">
                            {hats.map(item => {
                                const isWorn = item.catalogItemId === hatId
                                return (
                                    <button key={item.catalogItemId} type="button" aria-label={isWorn ? `Take off ${item.name}` : `Wear ${item.name}`} aria-pressed={isWorn} onClick={() => change({ hat: isWorn ? null : item })} className="game-card-cell cursor-pointer text-left">
                                        <div className={`game-card ${isWorn ? 'ring-2 ring-primary' : ''}`}>
                                            <CatalogItemCardBody item={item} isShopInfoShown={false} showOwnedCheckmark={false} />
                                        </div>
                                    </button>
                                )
                            })}
                        </div>
                    )}
                </section>
                <section>
                    <h2 className="mb-4 text-lg font-semibold tracking-tight">Color</h2>
                    <div className="flex flex-col gap-6 sm:flex-row sm:items-start">
                        <ColorField color={color} onChange={nextColor => change({ color: nextColor })} />
                        <div className="space-y-3">
                            <div className="grid w-fit grid-cols-6 gap-2.5 sm:grid-cols-4">
                                {QUICK_COLORS.map(quickColor => (
                                    <button
                                        key={quickColor.name}
                                        type="button"
                                        title={quickColor.name}
                                        aria-label={quickColor.name}
                                        aria-pressed={quickColor.color === color}
                                        onClick={() => change({ color: quickColor.color })}
                                        className={`size-8 cursor-pointer rounded-full shadow-sm ring-1 ring-black/10 transition-transform hover:scale-110 aria-pressed:ring-2 aria-pressed:ring-primary aria-pressed:ring-offset-2`}
                                        style={{ backgroundColor: quickColor.color }}
                                    />
                                ))}
                            </div>
                            <p className="font-mono text-sm text-gray-500">{color}</p>
                        </div>
                    </div>
                </section>
            </div>
            <div className={`mt-10 flex gap-3 transition duration-200 ${isChanged || isSaving ? '' : 'invisible translate-y-1 opacity-0'}`}>
                <Button type="button" disabled={isSaving} onClick={() => void save()}>{isSaving ? 'Saving…' : 'Save'}</Button>
                <Button type="button" variant="outline" disabled={isSaving} onClick={() => discardChanges(setDraft)}>Discard</Button>
            </div>
        </div>
    )
}
