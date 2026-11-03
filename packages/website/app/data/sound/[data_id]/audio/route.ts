import { readDataId, respondWithSound } from '@/lib/siteData'

export const GET = async (_request: Request, { params }: { params: Promise<{ data_id: string }> }) => {
    const dataId = readDataId((await params).data_id)
    if (dataId === null) return new Response(null, { status: 404 })
    return respondWithSound(dataId)
}
