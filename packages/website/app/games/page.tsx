import type { Metadata } from 'next'
import { JsonLd } from '@/components/site/JsonLd'
import { getRecentGames, getStaffPickGames, getTopGames } from '@/lib/siteGames'
import { toGameCardData } from '@/lib/siteGameCards'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, SITE_DESCRIPTION, getSitePageMetadata } from '@/lib/siteMetadata'
import { GAMES_PATH } from '@/lib/siteNavigation'
import { GamesPage } from './GamesPage'

export const metadata: Metadata = getSitePageMetadata({
    title: 'All Games',
    description: SITE_DESCRIPTION,
    path: GAMES_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

export default async function Page() {
    const [staffPickGames, topGames, recentGames] = await Promise.all([getStaffPickGames(), getTopGames(), getRecentGames()])

    return (
        <>
            <JsonLd value={getBreadcrumbJsonLd({ path: GAMES_PATH, pageName: 'All Games' })} />
            <GamesPage
                staffPickGames={staffPickGames.map(toGameCardData)}
                topGames={topGames.map(toGameCardData)}
                recentGames={recentGames.map(toGameCardData)}
            />
        </>
    )
}
