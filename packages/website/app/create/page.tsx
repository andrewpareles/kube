import type { Metadata } from 'next'
import { JsonLd } from '@/components/site/JsonLd'
import { getMyGames, getTopGames } from '@/lib/siteGames'
import { toGameCardData } from '@/lib/siteGameCards'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { ORGANIZATION_NAME, SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, SITE_DESCRIPTION, SITE_NAME, getSitePageMetadata, getSiteUrl } from '@/lib/siteMetadata'
import { CREATE_PATH } from '@/lib/siteNavigation'
import { CreatePage } from './CreatePage'

export const metadata: Metadata = getSitePageMetadata({
    title: 'New Game',
    description: 'Make a multiplayer game without coding.',
    path: CREATE_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

export default async function Page() {
    const [myGames, topGames] = await Promise.all([
        getMyGames(),
        getTopGames(),
    ])

    return (
        <>
            {/* The home page redirects here, so this page also says what the site and the company behind it are. */}
            <JsonLd value={[
                {
                    '@context': 'https://schema.org',
                    '@type': 'WebSite',
                    name: SITE_NAME,
                    url: getSiteUrl('/'),
                    description: SITE_DESCRIPTION,
                },
                {
                    '@context': 'https://schema.org',
                    '@type': 'Organization',
                    name: ORGANIZATION_NAME,
                    url: getSiteUrl('/'),
                    logo: getSiteUrl('/icon.png'),
                },
                getBreadcrumbJsonLd({ path: CREATE_PATH, pageName: 'New Game' }),
            ]} />
            <CreatePage
                myGames={myGames.map(toGameCardData)}
                allGames={topGames.map(toGameCardData)}
            />
        </>
    )
}
