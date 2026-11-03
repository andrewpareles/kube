'use client'

import { type ReactNode } from 'react'
import { X } from 'lucide-react'

type SiteModalFrameProps = {
    children: ReactNode
    isOpen: boolean
    onClose: () => void
}

export const ModalFrame = ({
    children,
    isOpen,
    onClose,
}: SiteModalFrameProps) => {
    if (!isOpen) {
        return null
    }

    return (
        <div className="fixed inset-0 z-[60] flex items-center justify-center px-6">
            {/* TODO: AI-PICKED-VALUE: The backdrop fades in, and the card fades in while growing from 95% of its size, both over 0.3 seconds and easing out. With reduced motion turned on, the card only fades. */}
            <div className="absolute inset-0 bg-black/5 backdrop-blur-[2px] animate-in fade-in animation-duration-300 ease-out" onClick={onClose} />

            <div className="site-surface-card flex flex-col animate-in fade-in motion-safe:zoom-in-95 animation-duration-300 ease-out">
                <button
                    type="button"
                    aria-label="Close modal"
                    onClick={onClose}
                    className="absolute right-4 top-4 rounded-full p-1.5 text-gray-400 transition-colors hover:bg-gray-100 hover:text-gray-950"
                >
                    <X className="size-4" />
                </button>

                {children}
            </div>
        </div>
    )
}
