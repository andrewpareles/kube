import { LuAudioLines } from 'react-icons/lu'
import type { AssetType } from '@/lib/assetTypes'
import { ASSETS_PATH } from '@/lib/siteNavigation'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'

// Both the directory and detail page use the same media preview.
// Sounds have no picture of their own, so they show an icon. Assets with a 3D preview show only the grey tile until it draws.
export const AssetPreview = ({ assetType, assetId, assetName, assetDataJson }: { assetType: AssetType, assetId: number, assetName: string, assetDataJson: unknown }) => {
    return (
        <div className="game-thumbnail bg-gradient-to-br from-gray-100 to-gray-200">
            <div className="absolute inset-0 flex items-center justify-center p-4">
                {assetType === 'Texture' ? (
                    <>
                        {/* eslint-disable-next-line @next/next/no-img-element -- The texture endpoint supplies sized PNGs; preserve their intrinsic dimensions. */}
                        <img src={`${ASSETS_PATH}/${assetId}/texture`} alt={assetName} loading="lazy" decoding="async" className="block h-auto w-auto max-h-full max-w-full rounded-md bg-white object-contain ring-1 ring-border" />
                    </>
                ) : (assetType === 'Sound' || assetType === 'Sfx') && (
                    <LuAudioLines className="size-10 text-gray-400" aria-hidden="true" />
                )}
            </div>
            {(assetType === 'Mesh' || assetType === 'Object' || assetType === 'Material' || assetType === 'Skybox') && <AssetScenePreview assetDataJson={JSON.stringify(assetDataJson)} assetName={assetName} mode="card" controls={null} footer={null} isInspectable={false} />}
        </div>
    )
}
