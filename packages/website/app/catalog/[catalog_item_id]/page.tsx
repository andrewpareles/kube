import Link from 'next/link'
import { notFound } from 'next/navigation'
import { cache } from 'react'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'
import { HumanWrittenNote } from '@/components/site/HumanWrittenNote'
import { displayedDescription } from '@/lib/siteDescriptions'
import { getCatalogItem } from '@/lib/siteCatalog'
import { PROFILE_PATH } from '@/lib/siteNavigation'
import { getUser } from '@/lib/siteUser'
import { CatalogItemActions, CatalogItemSettingsMenu } from './CatalogItemActions'

const getItem = cache(async (id: string) => {
    const catalogItemId = Number(id)
    if (!Number.isSafeInteger(catalogItemId) || catalogItemId <= 0) notFound()
    const user = await getUser()
    const item = await getCatalogItem({ catalogItemId, viewerId: user?.id ?? null })
    if (item === null) notFound()
    return { item, viewerId: user?.id ?? null }
})

export const generateMetadata = async ({ params }: { params: Promise<{ catalog_item_id: string }> }) => {
    const { item } = await getItem((await params).catalog_item_id)
    return { title: item.name, description: item.description ?? undefined }
}

export default async function Page({ params }: { params: Promise<{ catalog_item_id: string }> }) {
    const { item, viewerId } = await getItem((await params).catalog_item_id)
    const description = displayedDescription(item.description)

    return (
        <main className="site-container site-page">
            <div className="mb-6 flex items-start justify-between gap-6">
                <div className="min-w-0">
                    <h1 className="mb-2 break-words text-4xl font-semibold tracking-tight">{item.name}</h1>
                    <p className="text-sm text-gray-500">
                        By{' '}
                        <Link href={`${PROFILE_PATH}/${item.creatorUserId}`} className="transition-colors hover:text-gray-950">
                            {item.creatorUsername}
                        </Link>
                    </p>
                </div>
                {item.creatorId === viewerId && (
                    <CatalogItemSettingsMenu item={{ catalogItemId: item.catalogItemId, name: item.name, description: item.description, isListed: item.isListed }} />
                )}
            </div>
            <AssetScenePreview assetName={item.name} assetDataJson={JSON.stringify(item.dataJson)} mode="detail" footer={null} isInspectable={true} controls={
                <div className="flex min-w-0 flex-col gap-5 md:h-0 md:min-h-full">
                    {description && (
                        <div className="flex min-h-0 flex-col gap-1">
                            <p className="scroll-fade min-h-0 overflow-y-auto whitespace-pre-line break-words text-sm leading-6 text-gray-600">{description}</p>
                            <HumanWrittenNote />
                        </div>
                    )}
                    <CatalogItemActions
                        item={{ catalogItemId: item.catalogItemId, name: item.name, isListed: item.isListed }}
                        isOwned={item.isOwnedByViewer}
                        isSignedIn={viewerId !== null}
                    />
                </div>
            } />
        </main>
    )
}
