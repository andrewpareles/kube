import { GameDirectory, GameGrid } from '@/components/site/GameDirectory'
import { JsonLd } from '@/components/site/JsonLd'
import type { GameCardPlayData } from '@/components/site/GameCardPlay'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'

/**
 * One whole collection on its own page, which is where the rows on /games lead. There is no way
 * back on the page itself: the header's Games is the way back.
 */
export const GameCollectionPage = ({ title, path, games }: {
    title: string
    path: string
    games: GameCardPlayData[]
}) => (
    <>
        <JsonLd value={getBreadcrumbJsonLd({ path, pageName: title })} />
        <GameDirectory title={title} isTitleShown={true}>
            <GameGrid games={games} playerStat="live" />
        </GameDirectory>
    </>
)
