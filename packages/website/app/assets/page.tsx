import { ASSETS_PATH } from '@/lib/siteNavigation'
import type { Metadata } from 'next'
import { AssetCard } from '@/components/site/AssetCard'
import { MakeWithAiMenu } from '@/components/site/MakeWithAiMenu'
import { SitePagination } from '@/components/site/SitePagination'
import { ASSETS_PAGE_SIZE, assetTypeNames, countAssets, getAssets } from '@/lib/siteAssets'
import { assetTypes } from '@/lib/assetTypes'
import { Button } from '@/shadcn/components/ui/button'
import { Input } from '@/shadcn/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shadcn/components/ui/select'

export const metadata: Metadata = { title: 'Assets', description: 'Reusable meshes, textures, sounds, and objects for your games.' }

export default async function Page({ searchParams }: { searchParams: Promise<{ q?: string, type?: string, page?: string }> }) {
    const params = await searchParams
    const query = params.q ?? ''
    const assetType = assetTypes.find(type => assetTypeNames[type] === params.type)
    const assetTypeName = assetType === undefined ? undefined : assetTypeNames[assetType]
    const requestedPage = Number(params.page ?? '1')
    const page = Number.isSafeInteger(requestedPage) && requestedPage > 0 ? requestedPage : 1
    const [assets, assetCount] = await Promise.all([
        getAssets({ query, assetType, offset: (page - 1) * ASSETS_PAGE_SIZE, limit: ASSETS_PAGE_SIZE }),
        countAssets({ query, assetType }),
    ])
    const pageCount = Math.max(1, Math.ceil(assetCount / ASSETS_PAGE_SIZE))
    const pageHref = (nextPage: number) => `${ASSETS_PATH}?${new URLSearchParams({ q: query, type: assetTypeName ?? '', page: String(nextPage) })}`

    return (
        <main className="site-container site-page site-full-width-page">
            <div className="game-section-title flex items-center gap-1">
                <h1>Assets</h1>
                <div className="-my-2">
                    <MakeWithAiMenu menuLabel="Assets options" title="Make an Asset" description="To make an asset, ask for it in the chat." />
                </div>
            </div>
            <p className="site-page-description">Anyone can use, download, or publish an asset. It&apos;s completely optional; just for the benefit of the community.</p>
            {/* TODO: AI-PICKED-VALUE: Cap the search row at 42rem (max-w-2xl), room for the search box, type filter and button; the Catalog page uses the same width. */}
            <form role="search" className="my-6 flex max-w-2xl flex-wrap items-center gap-3" action={ASSETS_PATH}>
                <Input name="q" defaultValue={query} aria-label="Search Assets" placeholder="Search Assets" className="min-w-48 flex-1" />
                <Select key={assetTypeName ?? 'all'} name="type" defaultValue={assetTypeName ?? 'all'}>
                    <SelectTrigger aria-label="Asset type" className="w-40">
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        <SelectItem value="all">All Types</SelectItem>
                        {assetTypes.map(type => <SelectItem key={type} value={assetTypeNames[type]}>{assetTypeNames[type]}</SelectItem>)}
                    </SelectContent>
                </Select>
                <Button type="submit">Search</Button>
            </form>
            <SitePagination page={page} pageCount={pageCount} hrefOfPage={pageHref} label="Assets pages" className="mb-6" />
            <div className="site-game-grid">
                {assets.map(asset => <AssetCard key={asset.assetId} asset={asset} />)}
            </div>
            {assets.length === 0 && <p className="py-12 text-gray-500">No assets found.</p>}
            <SitePagination page={page} pageCount={pageCount} hrefOfPage={pageHref} label="Assets pages, bottom" className="mt-8" />
        </main>
    )
}
