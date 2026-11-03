import Link from 'next/link'
import { AssetPreview } from '@/components/site/AssetPreview'
import type { AssetType } from '@/lib/assetTypes'
import { assetTypeNames } from '@/lib/siteAssets'
import { getCondensedCount } from '@/lib/getCondensedCount'
import { ASSETS_PATH } from '@/lib/siteNavigation'

export const AssetCard = ({ asset }: { asset: { assetId: number, name: string, assetType: AssetType, dataJson: unknown, importCount: number } }) => (
    <Link href={`${ASSETS_PATH}/${asset.assetId}`} aria-label={`View ${asset.name}`} className="game-card-cell">
        <div className="game-card">
            <AssetPreview assetType={asset.assetType} assetId={asset.assetId} assetName={asset.name} assetDataJson={asset.dataJson} />
            <div className="flex items-start justify-between gap-3 py-2">
                <div className="min-w-0">
                    <h2 className="line-clamp-2 break-words font-[520] leading-[1.5] text-foreground text-sm">{asset.name}</h2>
                    <p className="text-xs text-gray-500">{assetTypeNames[asset.assetType]}</p>
                </div>
                <span className="shrink-0 leading-[1.5] text-gray-500 text-sm">{getCondensedCount(asset.importCount + 1)} {asset.importCount > 0 ? 'imports' : 'import'}</span>
            </div>
        </div>
    </Link>
)
