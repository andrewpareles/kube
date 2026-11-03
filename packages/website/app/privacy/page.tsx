import type { Metadata } from 'next'
import { JsonLd } from '@/components/site/JsonLd'
import { SiteMarkdownPage } from '@/components/site/SiteMarkdownPage'
import { getWebsiteFilePath, readMarkdownPage } from '@/lib/markdown'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'

export const dynamic = 'force-static'

const privacyMarkdownFilePath = getWebsiteFilePath('app', 'privacy', 'privacy.md')

export const generateMetadata = async (): Promise<Metadata> => {
    const { title, description } = await readMarkdownPage(privacyMarkdownFilePath)
    return getSitePageMetadata({
        title,
        description,
        path: '/privacy',
        imageUrl: SITE_DEFAULT_IMAGE_PATH,
        imageAlt: SITE_DEFAULT_IMAGE_ALT,
    })
}

const PrivacyPage = async () => {
    const { title, markdown } = await readMarkdownPage(privacyMarkdownFilePath)

    return (
        <>
            <JsonLd value={getBreadcrumbJsonLd({ path: '/privacy', pageName: 'Privacy' })} />
            <SiteMarkdownPage title={title} markdown={markdown} />
        </>
    )
}

export default PrivacyPage
