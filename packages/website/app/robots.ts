import type { MetadataRoute } from 'next'
import { SITE_ORIGIN, getSiteUrl } from '@/lib/siteMetadata'

const robots = (): MetadataRoute.Robots => ({
    rules: {
        userAgent: '*',
        allow: '/',
        disallow: [
            '/api/edgegap/',
            '/api/get-client-update-info/',
            '/api/play-sessions/',
            '/api/stripe/',
            '/plans/return',
        ],
    },
    sitemap: getSiteUrl('/sitemap.xml'),
    host: SITE_ORIGIN,
})

export default robots
