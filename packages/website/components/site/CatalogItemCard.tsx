import Link from 'next/link'
import { Check } from 'lucide-react'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'
import { CATALOG_PATH } from '@/lib/siteNavigation'

export type CatalogCardItem = { catalogItemId: number, name: string, dataJson: unknown, creatorUsername: string }

// A shop shows that an item is free, or that the viewer owns it, with a check. An item someone already owns, like a hat on their profile, shows only its name.
export const CatalogItemCardBody = ({ item, isShopInfoShown, showOwnedCheckmark }: { item: CatalogCardItem, isShopInfoShown: boolean, showOwnedCheckmark: boolean }) => (
    <>
        <div className="game-thumbnail bg-gradient-to-br from-gray-100 to-gray-200">
            <AssetScenePreview assetDataJson={JSON.stringify(item.dataJson)} assetName={item.name} mode="card" controls={null} footer={null} isInspectable={false} />
        </div>
        <div className="flex items-start justify-between gap-3 py-2">
            <div className="min-w-0">
                <h2 className="line-clamp-2 break-words font-[520] leading-[1.5] text-foreground text-sm">{item.name}</h2>
                {isShopInfoShown && <p className="truncate text-xs text-gray-500">{showOwnedCheckmark ? 'Owned' : 'Free'}</p>}
            </div>
            {showOwnedCheckmark && <Check className="size-4 shrink-0 text-green-600" aria-hidden="true" />}
        </div>
    </>
)

export const CatalogItemCard = ({ item, isShopInfoShown, showOwnedCheckmark }: { item: CatalogCardItem, isShopInfoShown: boolean, showOwnedCheckmark: boolean }) => (
    <Link href={`${CATALOG_PATH}/${item.catalogItemId}`} aria-label={`View ${item.name}`} className="game-card-cell">
        <div className="game-card">
            <CatalogItemCardBody item={item} isShopInfoShown={isShopInfoShown} showOwnedCheckmark={showOwnedCheckmark} />
        </div>
    </Link>
)
