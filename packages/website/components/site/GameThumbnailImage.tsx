'use client'

import Image from 'next/image'
import { useState } from 'react'
import { Skeleton } from '@/shadcn/components/ui/skeleton'
import { cn } from '@/shadcn/lib/utils'

// Each image keeps its own loading state and uses the browser's native lazy loading, unless it is high priority, when it is
// fetched as soon as the page's HTML names it. Next resizes each picture to the size it is drawn at, since the originals are
// full screenshots of 1-2 MB.
export const GameThumbnailImage = ({
    name,
    thumbnailUrl,
    className,
    sizes,
    isHighPriority,
}: {
    name: string
    thumbnailUrl: string | null
    className: string
    sizes: string
    isHighPriority: boolean
}) => {
    const [loadedThumbnailUrl, setLoadedThumbnailUrl] = useState<string | null>(null)
    const [failedThumbnailUrl, setFailedThumbnailUrl] = useState<string | null>(null)
    const shouldShowImage = thumbnailUrl !== null && thumbnailUrl !== failedThumbnailUrl
    const shouldShowSkeleton = thumbnailUrl !== loadedThumbnailUrl

    if (!shouldShowImage) {
        return (
            <div
                role="img"
                aria-label={name}
                aria-busy={false}
                className={cn('relative h-full w-full bg-cover bg-center', className)}
            >
                <Skeleton aria-hidden={true} className="absolute inset-0 h-full w-full rounded-none" />
            </div>
        )
    }

    return (
        <div aria-busy={shouldShowSkeleton} className={cn('relative h-full w-full bg-cover bg-center', className)}>
            <Image
                src={thumbnailUrl}
                alt={name}
                fill={true}
                sizes={sizes}
                loading={isHighPriority ? 'eager' : 'lazy'}
                fetchPriority={isHighPriority ? 'high' : 'auto'}
                className={cn('object-cover object-center transition-opacity duration-200', shouldShowSkeleton ? 'opacity-0' : 'opacity-100')}
                draggable={true}
                onLoad={() => {
                    setLoadedThumbnailUrl(thumbnailUrl)
                }}
                onError={() => {
                    setFailedThumbnailUrl(thumbnailUrl)
                }}
            />
            {shouldShowSkeleton ? <Skeleton aria-hidden={true} className="pointer-events-none absolute inset-0 h-full w-full rounded-none" /> : null}
        </div>
    )
}
