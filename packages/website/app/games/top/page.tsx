import type { Metadata } from 'next'
import { getTopGames } from '@/lib/siteGames'
import { toGameCardData } from '@/lib/siteGameCards'
import { TOP_GAMES_PATH } from '@/lib/siteNavigation'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'
import { GameCollectionPage } from '../GameCollectionPage'

export const metadata: Metadata = getSitePageMetadata({
    title: 'Top Liked',
    description: 'The most upvoted games on kube.new.',
    path: TOP_GAMES_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

export default async function Page() {
    const topGames = await getTopGames()

    return <GameCollectionPage title="Top Liked" path={TOP_GAMES_PATH} games={topGames.map(toGameCardData)} />
}
