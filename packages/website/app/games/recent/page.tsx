import type { Metadata } from 'next'
import { getRecentGames } from '@/lib/siteGames'
import { toGameCardData } from '@/lib/siteGameCards'
import { RECENT_GAMES_PATH } from '@/lib/siteNavigation'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'
import { GameCollectionPage } from '../GameCollectionPage'

export const metadata: Metadata = getSitePageMetadata({
    title: 'Recently Created',
    description: 'The newest games on kube.new.',
    path: RECENT_GAMES_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

export default async function Page() {
    const recentGames = await getRecentGames()

    return <GameCollectionPage title="Recently Created" path={RECENT_GAMES_PATH} games={recentGames.map(toGameCardData)} />
}
