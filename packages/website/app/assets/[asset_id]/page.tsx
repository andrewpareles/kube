import { ASSETS_PATH, DATA_PATH, PROFILE_PATH } from '@/lib/siteNavigation'
import Link from 'next/link'
import { notFound } from 'next/navigation'
import { cache } from 'react'
import { Play } from 'lucide-react'
import { LuAudioLines } from 'react-icons/lu'
import { assetTypeNames, getAsset, getAssetDataReferences } from '@/lib/siteAssets'
import { getUser } from '@/lib/siteUser'
import { AssetDetailLayout } from '@/components/site/AssetDetailLayout'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'
import { AssetSettingsMenu } from './AssetSettingsMenu'

const getItem = cache(async (id: string) => {
    const assetId = Number(id)
    if (!Number.isSafeInteger(assetId) || assetId <= 0) notFound()
    const asset = await getAsset({ assetId })
    if (asset === null) notFound()
    return asset
})

export const generateMetadata = async ({ params }: { params: Promise<{ asset_id: string }> }) => {
    const asset = await getItem((await params).asset_id)
    return { title: asset.name }
}

export default async function Page({ params }: { params: Promise<{ asset_id: string }> }) {
    const asset = await getItem((await params).asset_id)
    const user = await getUser()
    const dataReferences = getAssetDataReferences(asset.dataJson)
    const referencedData = dataReferences.length === 0 ? null : (
        <details className="group text-sm text-gray-500">
            <summary className="flex cursor-pointer list-none items-center text-gray-400 [&::-webkit-details-marker]:hidden"><span className="w-4 shrink-0"><Play className="size-2.5 fill-current transition-transform group-open:rotate-90" aria-hidden="true" /></span>Referenced Data</summary>
            <ul className="mt-2 space-y-1 pl-4">
                {dataReferences.map(({ dataKind, dataId }) => <li key={`${dataKind}-${dataId}`}><Link href={`${DATA_PATH}/${dataKind}/${dataId}`} className="transition-colors hover:text-gray-950">{dataKind === 'mesh' ? 'Mesh' : dataKind === 'texture' ? 'Texture' : 'Sound'} Data - {dataId}</Link></li>)}
            </ul>
        </details>
    )

    return (
        <main className="site-container site-page">
            <div>
                <div className="mb-2 flex items-start justify-between gap-4">
                    <h1 className="min-w-0 break-words text-4xl font-semibold tracking-tight">{asset.name}</h1>
                    {user?.id === asset.uploadedBy && <AssetSettingsMenu assetId={asset.assetId} assetName={asset.name} />}
                </div>
                <p className="mb-6 text-sm text-gray-500">
                    {assetTypeNames[asset.assetType]} from{' '}
                    <Link href={`${PROFILE_PATH}/${asset.uploaderUserId}`} className="transition-colors hover:text-gray-950">
                        {asset.uploaderUsername}
                    </Link>
                </p>
                {asset.assetType === 'Texture' ? (
                    <AssetDetailLayout preview={
                        <div className="flex aspect-[var(--game-thumbnail-ratio)] w-full items-center justify-center rounded-lg bg-gradient-to-br from-gray-100 to-gray-200 p-4">
                            {/* eslint-disable-next-line @next/next/no-img-element -- Display the texture at its intrinsic dimensions, constrained to the page width. */}
                            <img src={`${ASSETS_PATH}/${asset.assetId}/texture`} alt={asset.name} decoding="async" className="block max-h-full max-w-full" />
                        </div>
                    } controls={null} footer={referencedData} />
                ) : asset.assetType === 'Object' || asset.assetType === 'Mesh' || asset.assetType === 'Material' || asset.assetType === 'Skybox' ? <AssetScenePreview assetName={asset.name} assetDataJson={JSON.stringify(asset.dataJson)} mode="detail" controls={null} footer={referencedData} isInspectable={true} /> : (
                    <AssetDetailLayout preview={<div className="flex aspect-[var(--game-thumbnail-ratio)] w-full items-center justify-center rounded-lg bg-gradient-to-br from-gray-100 to-gray-200"><LuAudioLines className="size-16 text-gray-400" aria-hidden="true" /></div>} controls={<audio className="block w-full" aria-label={`Play ${asset.name}`} controls controlsList="nodownload" preload="none" src={`${ASSETS_PATH}/${asset.assetId}/sound`} />} footer={referencedData} />
                )}
            </div>
        </main>
    )
}
