import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { cache } from 'react'
import { JsonLd } from '@/components/site/JsonLd'
import { getGameDetailIfAllowed, getPublishedGameServers } from '@/lib/siteGames'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'
import { getSitePageMetadata, getSiteUrl, SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH } from '@/lib/siteMetadata'
import { GameDetailPage } from './GameDetailPage'

const getGameIdFromParams = async (params: Promise<{ game_id: string }>) => {
    const { game_id: gameIdString } = await params
    const gameId = Number(gameIdString)
    if (!Number.isInteger(gameId)) notFound()
    return gameId
}

const getGameDetailIfAllowedCached = cache(async (gameId: number) => {
    const gameDetailIfAllowed = await getGameDetailIfAllowed(gameId)
    if (gameDetailIfAllowed === null) notFound()
    return gameDetailIfAllowed
})

// metadata (dynamic per game)
export const generateMetadata = async ({ params }: { params: Promise<{ game_id: string }> }): Promise<Metadata> => {
    const gameId = await getGameIdFromParams(params)
    const game = await getGameDetailIfAllowedCached(gameId)
    const thumbnailUuid = game.aiThumbnailUuid ?? game.screenshotUuid
    return getSitePageMetadata({
        title: game.name,
        description: game.description ?? `Play ${game.name} on kube.new.`,
        path: `/games/${game.gameId}`,
        imageUrl: urlOfGameThumbnail(thumbnailUuid) ?? SITE_DEFAULT_IMAGE_PATH,
        imageAlt: thumbnailUuid === null ? SITE_DEFAULT_IMAGE_ALT : `${game.name} thumbnail`,
    })
}

export default async function Page({
    params,
}: {
    params: Promise<{ game_id: string }>
}) {
    const gameId = await getGameIdFromParams(params)
    const game = await getGameDetailIfAllowedCached(gameId)
    const servers = await getPublishedGameServers(gameId)
    const concurrentPlayers = servers.reduce((countOfPlayers, server) => {
        return countOfPlayers + server.players.length
    }, 0)
    const gamePath = `/games/${game.gameId}`

    return (
        <>
            {game.isPublished && (
                <JsonLd value={[
                    {
                        '@context': 'https://schema.org',
                        '@type': 'VideoGame',
                        name: game.name,
                        description: game.description ?? undefined,
                        url: getSiteUrl(gamePath),
                        image: urlOfGameThumbnail(game.aiThumbnailUuid ?? game.screenshotUuid) ?? undefined,
                        datePublished: game.createdAt ?? undefined,
                        playMode: 'MultiPlayer',
                        numberOfPlayers: game.maxPlayers,
                        author: {
                            '@type': 'Person',
                            name: game.creatorUsername,
                            url: getSiteUrl(`/profile/${game.creatorUserId}`),
                        },
                    },
                    getBreadcrumbJsonLd({ path: gamePath, pageName: game.name }),
                ]} />
            )}
            <GameDetailPage
                game={{
                    gameId: game.gameId,
                    name: game.name,
                    description: game.description,
                    aiThumbnailUuid: game.aiThumbnailUuid,
                    screenshotUuid: game.screenshotUuid,
                    creatorUserId: game.creatorUserId,
                    creatorUsername: game.creatorUsername,
                    concurrentPlayers,
                    isLikedByViewer: game.isLikedByViewer,
                    isFavoritedByViewer: game.isFavoritedByViewer,
                    totalVisits: game.totalVisits,
                    suggestedMinPlayers: game.suggestedMinPlayers,
                    maxPlayers: game.maxPlayers,
                    isPublished: game.isPublished,
                    hasWorld: game.hasWorld,
                }}
                servers={servers}
                isOwner={game.isOwner}
            />
        </>
    )
}
