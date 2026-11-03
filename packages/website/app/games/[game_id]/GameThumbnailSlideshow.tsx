'use client'

import { useState } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { GameThumbnailImage } from '@/components/site/GameThumbnailImage'
import { cn } from '@/shadcn/lib/utils'

const arrowClass = 'absolute top-1/2 flex size-9 -translate-y-1/2 items-center justify-center rounded-full bg-black/40 text-white backdrop-blur-sm transition-colors hover:bg-black/60'

// The pictures sit on top of each other, and the arrows fade to the next one, looping around after the last.
export const GameThumbnailSlideshow = ({ name, thumbnailUrls, sizes }: { name: string, thumbnailUrls: string[], sizes: string }) => {
    const [shownIndex, setShownIndex] = useState(0)
    if (thumbnailUrls.length < 2) {
        return <GameThumbnailImage name={name} thumbnailUrl={thumbnailUrls[0] ?? null} sizes={sizes} className="bg-gradient-to-br from-gray-100 to-gray-200" isHighPriority={true} />
    }
    const showNextBy = (step: number) => setShownIndex(index => (index + step + thumbnailUrls.length) % thumbnailUrls.length)
    return <>
        {thumbnailUrls.map((thumbnailUrl, index) => (
            <GameThumbnailImage
                key={thumbnailUrl}
                name={name}
                thumbnailUrl={thumbnailUrl}
                sizes={sizes}
                className={cn('absolute inset-0 bg-gradient-to-br from-gray-100 to-gray-200 transition-opacity duration-300', index === shownIndex ? 'opacity-100' : 'opacity-0')}
                isHighPriority={index === 0}
            />
        ))}
        <button type="button" aria-label="Previous picture" className={cn(arrowClass, 'left-3')} onClick={() => showNextBy(-1)}><ChevronLeft className="size-5" /></button>
        <button type="button" aria-label="Next picture" className={cn(arrowClass, 'right-3')} onClick={() => showNextBy(1)}><ChevronRight className="size-5" /></button>
    </>
}
