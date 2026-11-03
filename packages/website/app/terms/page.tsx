import type { Metadata } from 'next'
import { JsonLd } from '@/components/site/JsonLd'
import { SiteMarkdownPage } from '@/components/site/SiteMarkdownPage'
import { getWebsiteFilePath, readMarkdownPage } from '@/lib/markdown'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'

export const dynamic = 'force-static'

const termsMarkdownFilePath = getWebsiteFilePath('app', 'terms', 'terms.md')

export const generateMetadata = async (): Promise<Metadata> => {
    const { title, description } = await readMarkdownPage(termsMarkdownFilePath)
    return getSitePageMetadata({
        title,
        description,
        path: '/terms',
        imageUrl: SITE_DEFAULT_IMAGE_PATH,
        imageAlt: SITE_DEFAULT_IMAGE_ALT,
    })
}

const TermsPage = async () => {
    const { title, markdown } = await readMarkdownPage(termsMarkdownFilePath)

    return (
        <>
            <JsonLd value={getBreadcrumbJsonLd({ path: '/terms', pageName: 'Terms' })} />
            <SiteMarkdownPage title={title} markdown={markdown} />
        </>
    )
}

export default TermsPage
