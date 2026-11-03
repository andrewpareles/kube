import type { Metadata } from 'next'

export const SITE_NAME = 'kube.new'
export const SITE_ORIGIN = 'https://kube.new'
export const SITE_DESCRIPTION = 'Build and play games without coding.'
export const SITE_DEFAULT_IMAGE_PATH = '/opengraph-image.jpg'
export const SITE_DEFAULT_IMAGE_ALT = 'kube.new — Build multiplayer games without coding'
export const ORGANIZATION_NAME = 'Glass Devtools, Inc.'

export const getSiteUrl = (path: string) => new URL(path, SITE_ORIGIN).toString()

// Public pages use one metadata shape so their canonical and social URLs cannot drift apart.
export const getSitePageMetadata = ({ title, description, path, imageUrl, imageAlt }: {
    title: string
    description: string | null
    path: string
    imageUrl: string
    imageAlt: string
}): Metadata => {
    const socialTitle = `${title} | ${SITE_NAME}`
    return {
        title,
        description,
        alternates: { canonical: path },
        openGraph: {
            type: 'website',
            locale: 'en_US',
            siteName: SITE_NAME,
            title: socialTitle,
            description: description ?? undefined,
            url: path,
            images: [{ url: imageUrl, alt: imageAlt }],
        },
        twitter: {
            card: 'summary_large_image',
            title: socialTitle,
            description: description ?? undefined,
            images: [{ url: imageUrl, alt: imageAlt }],
        },
    }
}

// Articles add their publication date to the same canonical and social metadata as other pages.
export const getSiteArticleMetadata = ({ title, description, path, imageUrl, imageAlt, publishedTime }: {
    title: string
    description: string
    path: string
    imageUrl: string
    imageAlt: string
    publishedTime: string
}): Metadata => {
    const socialTitle = `${title} | ${SITE_NAME}`
    return {
        title,
        description,
        alternates: { canonical: path },
        openGraph: {
            type: 'article',
            locale: 'en_US',
            siteName: SITE_NAME,
            title: socialTitle,
            description,
            url: path,
            publishedTime,
            images: [{ url: imageUrl, alt: imageAlt }],
        },
        twitter: {
            card: 'summary_large_image',
            title: socialTitle,
            description,
            images: [{ url: imageUrl, alt: imageAlt }],
        },
    }
}

// Workflow pages are useful to people following a link, but they are not search results.
export const getNoIndexPageMetadata = ({ title, description }: { title: string; description: string | null }): Metadata => ({
    title,
    description,
    robots: { index: false, follow: false },
})
