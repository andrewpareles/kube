import { notFound } from 'next/navigation'
import { DATA_PATH } from '@/lib/siteNavigation'
import { readDataId, readSoundInfo } from '@/lib/siteData'

export const generateMetadata = async ({ params }: { params: Promise<{ data_id: string }> }) => ({ title: `Sound Data - ${(await params).data_id}` })

export default async function Page({ params }: { params: Promise<{ data_id: string }> }) {
    const dataId = readDataId((await params).data_id)
    const sound = dataId === null ? null : await readSoundInfo(dataId)
    if (dataId === null || sound === null) notFound()

    return (
        <main className="site-container site-page">
            <h1 className="mb-2 text-4xl font-semibold tracking-tight">Sound Data - {dataId}</h1>
            <p className="mb-1 text-sm text-gray-500">{sound.durationSeconds.toFixed(1)} s · {sound.sampleRate.toLocaleString()} Hz · {sound.channelCount === 1 ? 'Mono' : sound.channelCount === 2 ? 'Stereo' : `${sound.channelCount} channels`}</p>
            <p className="mb-6 text-sm text-gray-500">This shows up in the editor as Data Id {dataId}.</p>
            <audio className="block w-full max-w-sm" aria-label={`Play Sound Data - ${dataId}`} controls controlsList="nodownload" preload="none" src={`${DATA_PATH}/sound/${dataId}/audio`} />
        </main>
    )
}
