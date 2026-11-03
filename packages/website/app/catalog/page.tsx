import type { Metadata } from 'next'
import { CatalogItemCard } from '@/components/site/CatalogItemCard'
import { MakeWithAiMenu } from '@/components/site/MakeWithAiMenu'
import { SitePagination } from '@/components/site/SitePagination'
import { CATALOG_PAGE_SIZE, countCatalogItems, getCatalogItems } from '@/lib/siteCatalog'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'
import { CATALOG_PATH } from '@/lib/siteNavigation'
import { getUser } from '@/lib/siteUser'
// import { Button } from '@/shadcn/components/ui/button'
// import { Input } from '@/shadcn/components/ui/input'

export const metadata: Metadata = getSitePageMetadata({
    title: 'Catalog',
    description: 'Get free items to customize your character.',
    path: CATALOG_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

export default async function Page({ searchParams }: { searchParams: Promise<{ q?: string, page?: string }> }) {
    const params = await searchParams
    const query = params.q ?? ''
    const requestedPage = Number(params.page ?? '1')
    const page = Number.isSafeInteger(requestedPage) && requestedPage > 0 ? requestedPage : 1
    const user = await getUser()
    const [items, itemCount] = await Promise.all([
        getCatalogItems({ query, offset: (page - 1) * CATALOG_PAGE_SIZE, limit: CATALOG_PAGE_SIZE, viewerId: user?.id ?? null }),
        countCatalogItems({ query }),
    ])
    const pageCount = Math.max(1, Math.ceil(itemCount / CATALOG_PAGE_SIZE))
    const pageHref = (nextPage: number) => `${CATALOG_PATH}?${new URLSearchParams({ q: query, page: String(nextPage) })}`

    return (
        <main className="site-container site-page site-full-width-page">
            <div className="game-section-title flex items-center gap-1">
                <h1>Catalog</h1>
                <div className="-my-2">
                    <MakeWithAiMenu menuLabel="Catalog options" title="Make a Catalog Item" description="To make a catalog item, ask for it in the chat." />
                </div>
            </div>
            <p className="site-page-description mb-6">
                Make your own hat or equip someone else&apos;s! All hats are free.
            </p>
            {/* TODO: AI-PICKED-VALUE: Cap the search row at 42rem (max-w-2xl), the same width as the Assets page. */}
            {/* <form role="search" className="my-6 flex max-w-2xl flex-wrap items-center gap-3" action={CATALOG_PATH}>
                <Input name="q" defaultValue={query} aria-label="Search the Catalog" placeholder="Search the Catalog" className="min-w-48 flex-1" />
                <Button type="submit">Search</Button>
            </form> */}
            <SitePagination page={page} pageCount={pageCount} hrefOfPage={pageHref} label="Catalog pages" className="mb-6" />
            <div className="site-game-grid">
                {items.map(item => <CatalogItemCard key={item.catalogItemId} item={item} isShopInfoShown={true} showOwnedCheckmark={item.isOwnedByViewer} />)}
            </div>
            {items.length === 0 && <p className="py-12 text-gray-500">No items found.</p>}
            <SitePagination page={page} pageCount={pageCount} hrefOfPage={pageHref} label="Catalog pages, bottom" className="mt-8" />
        </main>
    )
}
