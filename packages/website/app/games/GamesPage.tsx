import type { ReactNode } from 'react'
import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import type { GameCardPlayData } from '@/components/site/GameCardPlay'
import { GameDirectory, GameGrid } from '@/components/site/GameDirectory'
import { RECENT_GAMES_PATH, TOP_GAMES_PATH } from '@/lib/siteNavigation'

// TODO: AI-PICKED-VALUE: Recent gets 5 games, the most columns the full-width grid has; site.css hides the ones past the first row.
const RECENT_ROW_GAME_COUNT = 5
// Playing Now fills its row with these, in this order, after the games people are in right now.
const PLAYING_NOW_DEFAULT_GAME_IDS = [343, 334, 332, 337, 336]

const GameSection = ({ title, href, children }: { title: string, href: string | null, children: ReactNode }) => (
    <section>
        <h2 className="game-section-title">
            {href === null ? title : <Link href={href} className="game-section-link">{title} <ArrowRight className="game-section-arrow shrink-0" aria-hidden="true" /></Link>}
        </h2>
        {children}
    </section>
)

// Top, newest, and the remaining ranked games share one grid, with Recent on its own route.
export const GamesPage = ({ staffPickGames, topGames, recentGames }: {
    staffPickGames: GameCardPlayData[]
    topGames: GameCardPlayData[]
    recentGames: GameCardPlayData[]
}) => {
    // Playing Now is the games people are in right now, most players first, then the default picks.
    const playingNowDefaultGames = PLAYING_NOW_DEFAULT_GAME_IDS.flatMap((gameId) => topGames.filter((game) => game.gameId === gameId && game.concurrentPlayers === 0))
    const playingNowGames = [...topGames.filter((game) => game.concurrentPlayers > 0), ...playingNowDefaultGames].slice(0, RECENT_ROW_GAME_COUNT)

    return (
        <GameDirectory title="All Games" isTitleShown={false}>
            <div className="flex flex-col gap-8">
                <GameSection title="Staff Picks" href={null}>
                    <div className="game-grid-single-row">
                        <GameGrid games={staffPickGames} playerStat="live" />
                    </div>
                </GameSection>
                <GameSection title="Playing Now" href={null}>
                    <div className="game-grid-single-row">
                        <GameGrid games={playingNowGames} playerStat="recent" />
                    </div>
                </GameSection>
                <GameSection title="Recently Created" href={RECENT_GAMES_PATH}>
                    <div className="game-grid-single-row">
                        <GameGrid games={recentGames.slice(0, RECENT_ROW_GAME_COUNT)} playerStat="live" />
                    </div>
                </GameSection>
                <GameSection title="Top Liked" href={TOP_GAMES_PATH}>
                    <GameGrid games={topGames} playerStat="live" />
                </GameSection>
            </div>
        </GameDirectory>
    )
}

// Earlier collection layout notes are retained while the layout above follows the prototype.
// Each collection has a short preview and a link to its full list.
// Recommended shows two rows of picks, followed by the ranked games that keep loading on scroll.
// The three collections you can switch between from the row of pills beside the heading.
// Recommended is the one you land on, and it is the only one made of two shelves: the games we
// picked out by hand, and then the most liked ones under them. Top and Recent are each just their
// own shelf.
// Our Picks is a single row at the head of Recommended, a glance at the few we chose, and hands
// anyone who wants the rest of them over to its own page. Top underneath it is the shelf you can
// keep scrolling.
// Every shelf on this page is the same row of play cards, so which games go in it, how far it is
// allowed to run, and where its heading leads are the only things that change between them.
// The one you are on is the filled pill, and the other two are only words until you point at them,
// so there is never any doubt about which collection is on the page below.
/* The heading sits at one end of the row and the selector at the other, both
                    lined up with the edges of the cards below them. They wrap onto separate lines
                    when there is not enough room across for the two of them. */
/* Pulled out by the pill's own padding, so the last word ends at the same
                        place the last card below it does. */
/* Every heading leads to that collection's own page, whether the shelf under it
                    is cut short or is the whole of what there is. */
