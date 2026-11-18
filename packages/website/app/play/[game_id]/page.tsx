import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { getJoinGameInfoAnyGame } from '@/app/server-actions/edgegap-actions'
import { PlayScene } from './PlayScene'

export const metadata: Metadata = {
    title: 'Play',
    robots: { index: false, follow: false },
    referrer: 'no-referrer',
}

// The page reserves this player a seat in the game, then joins the server the site started for it.
const Page = async ({ params }: { params: Promise<{ game_id: string }> }) => {
    const { game_id: gameIdParam } = await params
    if (!/^\d+$/.test(gameIdParam)) notFound()
    const joinInfo = await getJoinGameInfoAnyGame(Number.parseInt(gameIdParam, 10))

    return <PlayScene requestId={joinInfo.requestId} playAuth={joinInfo.playAuth} />
}

export default Page
