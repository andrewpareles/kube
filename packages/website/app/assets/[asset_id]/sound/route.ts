import { getAsset, readAssetDetails } from '@/lib/siteAssets'
import { respondWithSound } from '@/lib/siteData'

// Serve playable Ogg data from the engine asset so browsers do not need a bincode decoder.
export const GET = async (_request: Request, { params }: { params: Promise<{ asset_id: string }> }) => {
    const assetId = Number((await params).asset_id)
    if (!Number.isSafeInteger(assetId) || assetId <= 0) return new Response(null, { status: 404 })
    const asset = await getAsset({ assetId })
    if (asset === null || (asset.assetType !== 'Sound' && asset.assetType !== 'Sfx')) return new Response(null, { status: 404 })
    const details = readAssetDetails(asset.dataJson)
    if ('Sound' in details) return respondWithSound(details.Sound)
    if ('Sfx' in details) return respondWithSound(details.Sfx)
    throw new Error('Asset sound payload does not match its type')
}
