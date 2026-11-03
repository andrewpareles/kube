'use client'

import { memo, useEffect, useEffectEvent, useRef, useState } from 'react'
import Link from 'next/link'
import { LoaderCircle, Plus } from 'lucide-react'
import { toast } from 'sonner'
import { searchAssets } from '@/app/server-actions/assets'
import { AssetPreview } from '@/components/site/AssetPreview'
import { assetTypes, type AssetType } from '@/lib/assetTypes'
import { ASSETS_PATH } from '@/lib/siteNavigation'
import { Button } from '@/shadcn/components/ui/button'
import { Input } from '@/shadcn/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shadcn/components/ui/select'
import { EditorSidePanelHeader } from './EditorSidePanel'

type AssetSearch = { query: string, assetType: AssetType | undefined }
type AssetResults = { search: AssetSearch, items: Awaited<ReturnType<typeof searchAssets>>['items'], hasMore: boolean }

export const EditorAssetsPanel = memo(({ editor, onClose }: { editor: { addAsset: (assetId: number) => Promise<void> } | null, onClose: () => void }) => {
    const [query, setQuery] = useState('')
    const [assetType, setAssetType] = useState<AssetType | 'all'>('all')
    const [search, setSearch] = useState<AssetSearch>({ query: '', assetType: undefined })
    const [results, setResults] = useState<AssetResults | null>(null)
    const [isLoadingMore, setIsLoadingMore] = useState(false)
    const [addingIds, setAddingIds] = useState<number[]>([])
    const scrollerRef = useRef<HTMLDivElement>(null)
    const loadMoreRef = useRef<HTMLDivElement>(null)
    useEffect(() => {
        let isCurrent = true
        searchAssets({ ...search, offset: 0 }).then(page => { if (isCurrent) setResults({ search, ...page }) }, error => { if (isCurrent) toast.error(String(error), { id: 'editor-assets' }) })
        return () => { isCurrent = false }
    }, [search])
    const loadMore = useEffectEvent(() => {
        if (!results?.hasMore || results.search !== search || isLoadingMore) return
        setIsLoadingMore(true)
        searchAssets({ ...search, offset: results.items.length }).then(page => {
            // Offsets shift when something is published between pages, so an item can come back.
            setResults(previous => {
                if (previous?.search !== search) return previous
                const shownIds = new Set(previous.items.map(item => item.assetId))
                return { search, items: [...previous.items, ...page.items.filter(item => !shownIds.has(item.assetId))], hasMore: page.hasMore }
            })
            setIsLoadingMore(false)
        }, error => {
            // The end of the list is still in view, so loading stops until the next search instead of retrying forever.
            toast.error(String(error), { id: 'editor-assets' })
            setResults(previous => previous?.search === search ? { ...previous, hasMore: false } : previous)
            setIsLoadingMore(false)
        })
    })
    // Observing again after every page reports the end of the list again when it is still in view.
    // TODO: AI-PICKED-VALUE: Load the next page 200px before the end of the list scrolls into view.
    useEffect(() => {
        const scroller = scrollerRef.current, end = loadMoreRef.current
        if (!scroller || !end) return
        const observer = new IntersectionObserver(([entry]) => { if (entry.isIntersecting) loadMore() }, { root: scroller, rootMargin: '200px' })
        observer.observe(end)
        return () => observer.disconnect()
    }, [results, isLoadingMore])
    const addAsset = async (assetId: number) => {
        if (!editor) return
        setAddingIds(previous => [...previous, assetId])
        try { await editor.addAsset(assetId) } finally { setAddingIds(previous => previous.filter(id => id !== assetId)) }
    }
    return <section aria-label="Assets" className="flex h-full min-h-0 flex-col">
        <EditorSidePanelHeader title="Assets" onClose={onClose} />
        <form className="flex flex-wrap items-center gap-2 px-3 pb-2" onSubmit={event => { event.preventDefault(); setSearch({ query, assetType: assetType === 'all' ? undefined : assetType }) }}>
            <Input value={query} onChange={event => setQuery(event.target.value)} aria-label="Search Assets" placeholder="Search Assets" autoComplete="off" data-lpignore="true" className="h-8 min-w-36 flex-1" />
            <Select value={assetType} onValueChange={value => setAssetType(assetTypes.find(type => type === value) ?? 'all')}>
                <SelectTrigger size="sm" aria-label="Asset type" className="w-32">
                    <SelectValue />
                </SelectTrigger>
                <SelectContent>
                    <SelectItem value="all">All types</SelectItem>
                    {assetTypes.map(type => <SelectItem key={type} value={type}>{type}</SelectItem>)}
                </SelectContent>
            </Select>
            <Button type="submit" size="sm">Search</Button>
        </form>
        {/* game-card hovers reach 12px past the sides and 8px past the top and bottom of each card. */}
        <div ref={scrollerRef} className="@container min-h-0 flex-1 overflow-y-auto px-3 py-2">
            <div className="grid grid-cols-1 items-start gap-x-7.5 gap-y-6 @[20rem]:grid-cols-2">
                {/* Each card opens the asset's page in a new tab. A button cannot sit inside a link, so the add button sits beside it and shows while the card is hovered or focused. */}
                {results?.items.map(asset => {
                    const isAdding = addingIds.includes(asset.assetId)
                    return <div key={asset.assetId} className="game-card group">
                        <Link href={`${ASSETS_PATH}/${asset.assetId}`} target="_blank" aria-label={`View ${asset.name}`} className="block">
                            <AssetPreview assetType={asset.assetType} assetId={asset.assetId} assetName={asset.name} assetDataJson={asset.dataJson} />
                            <div className="py-2">
                                <h2 className="break-words font-[520] leading-[1.5] text-foreground text-sm">{asset.name}</h2>
                            </div>
                        </Link>
                        <div className="pointer-events-none absolute inset-x-[var(--game-card-padding)] top-[var(--game-card-padding)] aspect-[var(--game-thumbnail-ratio)]">
                            <Button type="button" size="icon" aria-label={`Add ${asset.name}`} aria-busy={isAdding} disabled={!editor || isAdding} onClick={() => void addAsset(asset.assetId)} className={`pointer-events-auto absolute bottom-2 right-2 size-8 bg-black text-white hover:bg-black/80 group-hover:opacity-100 group-focus-within:opacity-100 ${isAdding ? 'opacity-100' : 'opacity-0'}`}>{isAdding ? <LoaderCircle className="animate-spin" /> : <Plus />}</Button>
                        </div>
                    </div>
                })}
            </div>
            {results?.items.length === 0 && <p className="py-6 text-xs text-gray-500">No assets found.</p>}
            {(results?.search !== search || isLoadingMore) && <div role="status" aria-label="Loading assets" className="flex justify-center py-3"><LoaderCircle aria-hidden="true" className="size-4 animate-spin text-gray-400" /></div>}
            <div ref={loadMoreRef} />
        </div>
    </section>
})
EditorAssetsPanel.displayName = 'EditorAssetsPanel'
