'use client'

import type { ReactNode } from 'react'
import { GameCardPlay, type GameCardPlayData } from '@/components/site/GameCardPlay'

// On the games page the name is only for screen readers and search engines, since every row has its own name. A collection's own page
// shows its name the way its row does.
export const GameDirectory = ({ title, isTitleShown, children }: {
    title: string
    isTitleShown: boolean
    children: ReactNode
}) => {
    return (
        <main className="site-container site-page site-full-width-page">
            <h1 className={isTitleShown ? 'game-section-title' : 'sr-only'}>{title}</h1>
            <div id="game-results">
                {children}
            </div>
        </main>
    )
}

export const GameGrid = ({ games, playerStat }: { games: GameCardPlayData[], playerStat: 'live' | 'recent' }) => (
    games.length === 0 ? (
        <p role="status" className="py-16 text-sm text-muted-foreground">No games yet.</p>
    ) : (
        <div className="site-game-grid">
            {games.map((game) => <GameCardPlay key={game.gameId} game={game} playerStat={playerStat} />)}
        </div>
    )
)
