import type { MetadataRoute } from 'next'
import { getBlogPosts } from '@/lib/blog'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'
import { getRecentGames } from '@/lib/siteGames'
import { getSiteUrl } from '@/lib/siteMetadata'
import { BLOG_PATH, ASSETS_PATH, CREATE_PATH, DOCS_PATH, DOWNLOAD_PATH, GAMES_PATH, PRIVACY_PATH, PROFILE_PATH, RECENT_GAMES_PATH, TERMS_PATH, TOP_GAMES_PATH } from '@/lib/siteNavigation'

export const dynamic = 'force-dynamic'

const sitemap = async (): Promise<MetadataRoute.Sitemap> => {
    const [blogPosts, games] = await Promise.all([getBlogPosts(), getRecentGames()])
    const staticPaths = [
        CREATE_PATH,
        GAMES_PATH,
        ASSETS_PATH,
        TOP_GAMES_PATH,
        RECENT_GAMES_PATH,
        BLOG_PATH,
        DOCS_PATH,
        DOWNLOAD_PATH,
        PRIVACY_PATH,
        TERMS_PATH,
    ]
    const sitemapEntries: MetadataRoute.Sitemap = staticPaths.map((path) => ({ url: getSiteUrl(path) }))

    for (const blogPost of blogPosts) {
        if (!blogPost.isPublished) continue
        sitemapEntries.push({
            url: getSiteUrl(`${BLOG_PATH}/${blogPost.slug}`),
            lastModified: blogPost.date,
            images: [getSiteUrl(blogPost.thumbnailImageUrl)],
        })
    }

    const isProfileIncludedOfUserId: { [userId: number]: true } = {}
    for (const game of games) {
        const thumbnailUrl = urlOfGameThumbnail(game.thumbnailUuid)
        const gameEntry: MetadataRoute.Sitemap[number] = {
            url: getSiteUrl(`/games/${game.gameId}`),
            images: thumbnailUrl === null ? [] : [thumbnailUrl],
        }
        if (game.createdAt !== null) gameEntry.lastModified = game.createdAt
        sitemapEntries.push(gameEntry)

        if (isProfileIncludedOfUserId[game.creatorUserId] === true) continue
        isProfileIncludedOfUserId[game.creatorUserId] = true
        const profilePath = `${PROFILE_PATH}/${game.creatorUserId}`
        sitemapEntries.push({ url: getSiteUrl(profilePath) })
    }

    return sitemapEntries
}

export default sitemap
