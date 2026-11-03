import type { Metadata } from 'next'
import Link from 'next/link'
import { notFound } from 'next/navigation'
import { EyeOff } from 'lucide-react'
import { GameThumbnailImage } from '@/components/site/GameThumbnailImage'
import { GAME_THUMBNAIL_SIZES } from '@/lib/gameRowLayout'
import { getAllGamesIfAdmin, type SiteGameSummary } from '@/lib/siteGames'
import { EDITOR_PATH } from '@/lib/siteNavigation'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'

export const metadata: Metadata = {
    title: 'Admin',
    robots: { index: false, follow: false },
}

// How long ago a game was made, in the biggest whole unit, so the newest games read at a glance.
const getTimeAgo = (createdAt: string) => {
    const minutes = Math.floor((Date.now() - new Date(createdAt).getTime()) / 60000)
    if (minutes < 60) return `${minutes}m ago`
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return `${hours}h ago`
    return `${Math.floor(hours / 24)}d ago`
}

// Each card opens the game's editor, the same way an invite link would.
const AdminGameCard = ({ game }: { game: SiteGameSummary }) => (
    <Link href={EDITOR_PATH(game.gameId)} aria-label={`Open ${game.name} in the editor`} className="game-card-cell">
        <div className="game-card">
            <div className="game-thumbnail">
                <GameThumbnailImage name={game.name} thumbnailUrl={urlOfGameThumbnail(game.thumbnailUuid)} sizes={GAME_THUMBNAIL_SIZES} className="bg-gradient-to-br from-gray-100 to-gray-200" isHighPriority={false} />
            </div>
            <div className="flex items-start justify-between gap-3 py-2">
                <div className="min-w-0">
                    <p className="break-words font-[520] leading-[1.5] text-foreground text-sm">{game.name}</p>
                    <p className="text-xs text-gray-500">{game.creatorUsername}{game.createdAt !== null && <> · {getTimeAgo(game.createdAt)}</>}</p>
                </div>
                {!game.isPublished && <EyeOff className="mt-1 size-3.5 shrink-0 text-gray-500" />}
            </div>
        </div>
    </Link>
)

// Every game on the site, private ones too, newest first. Anyone who isn't a site admin gets a 404.
const Page = async () => {
    const games = await getAllGamesIfAdmin()
    if (games === null) notFound()

    return (
        <main className="site-container site-page site-full-width-page">
            <h1 className="site-page-title mb-10">All Games</h1>
            <div className="site-game-grid">
                {games.map((game) => <AdminGameCard key={game.gameId} game={game} />)}
            </div>
        </main>
    )
}

export default Page
