'use client'

import type { ReactNode } from 'react'
import { ArrowDown } from 'lucide-react'
import { useStickToBottom } from 'use-stick-to-bottom'
import { Button } from '@/shadcn/components/ui/button'

// Follow growing content until the user scrolls away; resizing Markdown and tool results count too.
export const EditorChatScroll = ({ children }: { children: ReactNode }) => {
    const { scrollRef, contentRef, isAtBottom, scrollToBottom } = useStickToBottom({ initial: 'instant', resize: 'instant' })
    return <div className="relative flex min-h-0 flex-1 flex-col">
        {/* Match the composer's margin plus padding horizontally, and keep the bottom gap inside the scrolling messages. */}
        <div ref={scrollRef} role="region" aria-label="Chat messages" tabIndex={0} className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 pt-3 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-gray-400">
            {/* On phones the Show Editor button floats over the bottom right, so the last lines can scroll up above it. */}
            <div ref={contentRef} className="space-y-3 pb-3 max-lg:pb-16">{children}</div>
        </div>
        {!isAtBottom && <Button type="button" variant="outline" size="icon" aria-label="Scroll to latest" className="absolute bottom-3 left-1/2 size-8 -translate-x-1/2 rounded-full bg-white shadow-sm" onClick={() => { void scrollToBottom({ animation: 'instant' }) }}><ArrowDown aria-hidden="true" className="size-4" /></Button>}
    </div>
}
