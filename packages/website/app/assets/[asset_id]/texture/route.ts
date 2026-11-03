import { getAsset, readAssetDetails } from '@/lib/siteAssets'
import { respondWithTexturePreview } from '@/lib/siteData'

// Serve a browser-readable preview of the existing engine asset without storing a second upload.
export const GET = async (request: Request, { params }: { params: Promise<{ asset_id: string }> }) => {
    const assetId = Number((await params).asset_id)
    if (!Number.isSafeInteger(assetId) || assetId <= 0) return new Response(null, { status: 404 })
    const asset = await getAsset({ assetId })
    if (asset === null || asset.assetType !== 'Texture') return new Response(null, { status: 404 })
    const details = readAssetDetails(asset.dataJson)
    if (!('Texture' in details)) throw new Error('Asset texture payload does not match its type')
    return respondWithTexturePreview(request, details.Texture)
}
