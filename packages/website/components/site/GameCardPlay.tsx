import Link from 'next/link'
import { EyeOff, Play } from 'lucide-react'
// import { ThumbsUp } from 'lucide-react'
import { GAME_PATH } from '@/app/deeplink/deeplinkFlowConsts'
import { GameThumbnailImage } from '@/components/site/GameThumbnailImage'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'
import { GAME_THUMBNAIL_SIZES } from '@/lib/gameRowLayout'
import { getCondensedCount } from '@/lib/getCondensedCount'
import { getPlayerRangeText } from '@/lib/getPlayerRangeText'

export type GameCardPlayData = {
    gameId: number
    name: string
    thumbnailUuid: string | null
    totalVisits: number
    suggestedMinPlayers: number
    maxPlayers: number
    concurrentPlayers: number
    isPublished: boolean
}

// A card shows how many people are playing it right now, or, on Playing Now, how many have played it recently. Recently is
// its total visits for now, since plays are not recorded with when they happened.
export const GameCardPlay = ({ game, playerStat }: { game: GameCardPlayData, playerStat: 'live' | 'recent' }) => {
    const gamePath = GAME_PATH(game.gameId)

    return (
        <Link
            href={gamePath}
            aria-label={`View ${game.name}`}
            className="game-card-cell"
        >
            <div className="game-card">
                {/* Thumbnail */}
                <div className="game-thumbnail">
                    <GameThumbnailImage
                        name={game.name}
                        thumbnailUrl={urlOfGameThumbnail(game.thumbnailUuid)}
                        sizes={GAME_THUMBNAIL_SIZES}
                        className="bg-gradient-to-br from-gray-100 to-gray-200"
                        isHighPriority={false}
                    />
                    {/* The dark wash fades on its own layer, by opacity rather than by colour, so it
                        never has to repaint in step with the thumbnail scaling underneath it. It stays
                        at its own 200ms against the thumbnail's slower 300ms, which is the pairing the
                        card was designed around: the zoom eases, the wash lands. */}
                </div>

                {/* Info */}
                <div className="flex items-start justify-between gap-3 py-2">
                    <div className="min-w-0">
                        <p className="line-clamp-2 break-words font-[520] leading-[1.5] text-foreground text-sm">{game.name}</p>
                        {/* <p className="text-xs text-gray-500">{getCondensedCount(game.totalVisits)} {game.totalVisits === 1 ? 'visit' : 'visits'}</p> */}
                        <p className="text-xs text-gray-500">{getPlayerRangeText(game)}</p>
                    </div>
                    <div className="flex h-lh shrink-0 items-center gap-2 leading-[1.5] text-gray-500 text-sm">
                        {playerStat === 'live' && game.concurrentPlayers > 0 && (
                            <span className="flex items-center gap-1 text-xs font-medium text-primary">
                                <Play className="size-3 fill-current" />
                                {getCondensedCount(game.concurrentPlayers)} live
                            </span>
                        )}
                        {playerStat === 'recent' && (
                            <span className="flex items-center gap-1.5 text-xs">
                                <span aria-hidden="true" className="size-1.5 rounded-full bg-red-500" />
                                {getCondensedCount(game.totalVisits)} recent
                            </span>
                        )}
                        {!game.isPublished && <EyeOff className="size-3.5" />}
                    </div>
                    {/* <div className="mt-1 flex items-center gap-3 text-xs text-gray-500">
                        <span className="flex items-center gap-1">
                            <ThumbsUp className="size-3.5" />
                            {game.likes.toLocaleString()}
                        </span>
                    </div> */}
                </div>
            </div>
        </Link>
    )
}

// Earlier shortcut behavior, now removed:
// The card opens the game's page; its separate button uses the same launch flow as that page.
