import { type ReactNode } from 'react'
import { SiteGamesBackdrop, type SiteBackdropGame } from '@/components/site/SiteGamesBackdrop'

/**
 * A single card held in the middle of the screen with the games scattered behind it. Signing in and
 * downloading the app both look like this, and they share the whole of it rather than each writing
 * out the same card, so the two cannot drift into looking almost alike.
 */
export const SiteBackdropCard = ({ backgroundGames, children }: {
    backgroundGames: SiteBackdropGame[]
    children: ReactNode
}) => (
    <main className="site-backdrop-page">
        <SiteGamesBackdrop games={backgroundGames} />
        <div className="relative w-full max-w-md">
            <div className="site-surface-card">
                {children}
            </div>
        </div>
    </main>
)
