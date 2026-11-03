'use client'

import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shadcn/components/ui/tooltip'

// A tooltip that is open exactly while a mouse is over its trigger, and nothing else. Radix's own
// tooltip also opens on keyboard focus, shuts on a click, and stays open while the pointer is over
// the tooltip. All of those work by asking to change the open state, so holding that state here and
// never listening to those asks leaves only the hover. A touch has no hover, so it never opens one.
//
// The tooltip lets the pointer pass straight through it. It slides in from the trigger's side, so for
// a moment it covers the trigger's edge, and a pointer resting there would otherwise land on the
// tooltip, count as leaving the trigger, shut it, land on the trigger again, and flicker.
// openDelayMs holds it back for a moment, for buttons whose label already says what they do.
// isShownOnPress also opens it while the trigger is pressed, so a tap on a phone shows it too. A touch then keeps it open after the finger
// lifts, until the next press somewhere else.
// A tap on a disabled button inside the trigger opens it the same way, since the tooltip is the only thing the tap can do. Disabled
// buttons let presses through to the trigger, because a browser sends no events to a disabled button.
export const HoverTooltip = ({ trigger, side, className, openDelayMs = 0, isShownOnPress = false, children }: { trigger: ReactNode, side: 'top' | 'right' | 'bottom' | 'left', className: string, openDelayMs?: number, isShownOnPress?: boolean, children: ReactNode }) => {
    const [isOpen, setIsOpen] = useState(false)
    const openTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
    const triggerRef = useRef<HTMLButtonElement>(null)
    useEffect(() => () => clearTimeout(openTimeoutRef.current), [])
    useEffect(() => {
        if (!isOpen) return
        const closeOnPressElsewhere = (event: PointerEvent) => {
            if (!(event.target instanceof Node && triggerRef.current?.contains(event.target))) setIsOpen(false)
        }
        document.addEventListener('pointerdown', closeOnPressElsewhere)
        return () => document.removeEventListener('pointerdown', closeOnPressElsewhere)
    }, [isOpen])

    return (
        <Tooltip open={isOpen}>
            <TooltipTrigger asChild ref={triggerRef} className="[&_:disabled]:pointer-events-none" onPointerEnter={(event) => {
                if (event.pointerType === 'touch') return
                clearTimeout(openTimeoutRef.current)
                if (openDelayMs === 0) setIsOpen(true)
                else openTimeoutRef.current = setTimeout(() => setIsOpen(true), openDelayMs)
            }} onPointerLeave={(event) => {
                if (event.pointerType === 'touch') return
                clearTimeout(openTimeoutRef.current)
                setIsOpen(false)
            }} onPointerDown={(event) => { if (isShownOnPress || (event.pointerType === 'touch' && event.currentTarget.querySelector(':disabled'))) setIsOpen(true) }}>
                {trigger}
            </TooltipTrigger>
            <TooltipContent side={side} className={`pointer-events-none ${className}`}>{children}</TooltipContent>
        </Tooltip>
    )
}
