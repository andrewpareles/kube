import { Children, type ReactNode } from 'react'
import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import { GAME_ROW_MAX_WIDTH_PX } from '@/lib/gameRowLayout'

// Two columns that split the row below sm, so the cards absorb the leftover width instead of it
// piling up on the outside edges; as many fixed 9rem columns as fit at sm and up. The title is laid
// out on these same columns and spans all of them, which is what starts it at the first card's left
// edge however many columns there turn out to be.
const GAME_ROW_COLUMN_CLASSES = 'site-game-grid'

type GameRowProps = {
    title: string
    titleHref: string | null
    headingLevel: 'page' | 'section' | 'none'
    children: ReactNode
}

// Render the supplied cards directly; each thumbnail loads and appears independently.
export const GameRow = ({
    title,
    titleHref,
    headingLevel,
    children,
}: GameRowProps) => {
    const gameNodes = Children.toArray(children)
    const headingContent = titleHref === null ? title : (
        <Link href={titleHref} className="game-section-link">
            <span>{title}</span>
            <ArrowRight className="game-section-arrow shrink-0" aria-hidden="true" />
        </Link>
    )

    return (
        <section aria-label={headingLevel === 'none' ? title : undefined}>
            <div className="pb-6">
                <div className="mx-auto w-full" style={{ maxWidth: GAME_ROW_MAX_WIDTH_PX }}>
                    {headingLevel !== 'none' && <div className={GAME_ROW_COLUMN_CLASSES}>
                        {headingLevel === 'page' ? (
                            <h1 className="site-page-title col-span-full mb-6">
                                {headingContent}
                            </h1>
                        ) : (
                            <h2 className="site-section-title col-span-full mb-6">
                                {headingContent}
                            </h2>
                        )}
                    </div>}

                    {gameNodes.length === 0 ? (
                        <p className="rounded-lg border border-gray-200 bg-gray-50 px-4 py-12 text-center text-sm text-gray-500">No games yet.</p>
                    ) : (
                        <div className={GAME_ROW_COLUMN_CLASSES}>
                            {gameNodes}
                        </div>
                    )}
                </div>
            </div>
        </section>
    )
}
