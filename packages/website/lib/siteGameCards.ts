import type { GameCardPlayData } from '@/components/site/GameCardPlay'
import type { SiteGameSummary } from '@/lib/siteGames'

/** Narrows a game from the database down to just what a card on a row needs to draw itself. */
export const toGameCardData = (game: SiteGameSummary): GameCardPlayData => ({
    gameId: game.gameId,
    name: game.name,
    thumbnailUuid: game.thumbnailUuid,
    totalVisits: game.totalVisits,
    suggestedMinPlayers: game.suggestedMinPlayers,
    maxPlayers: game.maxPlayers,
    concurrentPlayers: game.concurrentPlayers,
    isPublished: game.isPublished,
})
