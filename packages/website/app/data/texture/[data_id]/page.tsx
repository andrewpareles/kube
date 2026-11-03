import { notFound } from 'next/navigation'
import { DATA_PATH } from '@/lib/siteNavigation'
import { readDataId, readTextureInfo } from '@/lib/siteData'

export const generateMetadata = async ({ params }: { params: Promise<{ data_id: string }> }) => ({ title: `Texture Data - ${(await params).data_id}` })

export default async function Page({ params }: { params: Promise<{ data_id: string }> }) {
    const dataId = readDataId((await params).data_id)
    const texture = dataId === null ? null : await readTextureInfo(dataId)
    if (dataId === null || texture === null) notFound()

    return (
        <main className="site-container site-page">
            <h1 className="mb-2 text-4xl font-semibold tracking-tight">Texture Data - {dataId}</h1>
            <p className="mb-1 text-sm text-gray-500">{texture.width} × {texture.height} · {texture.isSrgb ? 'Color' : 'Data map'}</p>
            <p className="mb-6 text-sm text-gray-500">This shows up in the editor as Data Id {dataId}.</p>
            {/* eslint-disable-next-line @next/next/no-img-element -- Display the texture at its intrinsic dimensions, constrained to the page width. */}
            <img src={`${DATA_PATH}/texture/${dataId}/image`} alt={`Texture Data - ${dataId}`} decoding="async" className="block h-auto max-w-full" />
        </main>
    )
}
