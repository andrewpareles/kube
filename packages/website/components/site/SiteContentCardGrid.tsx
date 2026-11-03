import Image from 'next/image'
import Link from 'next/link'
import { restingLogoPreset } from '@/lib/kubeLogo/kubeLogoRender'
import type { ReactNode } from 'react'

// Which drawing a card is given when it has no image of its own.
export type SiteContentCardArtName = 'blog' | 'scripting' | 'ai'

// -------------------------- The art on a card with no image --------------------------

// A card with no image of its own is drawn with the striped splash the blog posts use, copied out
// of public/blog down to its path data, so these cards and the blog's own are the same picture.
// Only the pair of colours changes from card to card.
const cardSplashGroundColor = '#f8fafc'

const CardArtStripes = ({ bandColor, stripeColor }: { bandColor: string; stripeColor: string }) => (
    <svg viewBox="0 0 1200 520" preserveAspectRatio="xMidYMid slice" aria-hidden={true} className="pointer-events-none absolute inset-0 size-full">
        <rect width="1200" height="520" fill={cardSplashGroundColor} />
        <path d="M0 430 1200 120v400H0Z" fill={bandColor} />
        <path d="M0 120 460 0h210L0 175Z" fill={stripeColor} />
        <path d="M0 230 860 0h210L0 285Z" fill={stripeColor} />
        <path d="M170 520 1200 250v80L470 520Z" fill={stripeColor} />
        <path d="M640 520 1200 375v80L950 520Z" fill={stripeColor} />
    </svg>
)

// TODO: AI-PICKED-VALUE: the dark band under each pair is the deep end of the hue its stripes are
// drawn in, so the scripting card reads blue and the AI card reads cyan.
const cardArtOfArtName: { [artName in SiteContentCardArtName]: ReactNode } = {
    ai: <CardArtStripes bandColor="#0c4a6e" stripeColor={restingLogoPreset.gradientEndColor} />,
    blog: <CardArtStripes bandColor="#111827" stripeColor={restingLogoPreset.glowColor} />,
    scripting: <CardArtStripes bandColor="#1e3a8a" stripeColor={restingLogoPreset.glowColor} />,
}

export type SiteContentCard = {
    href: string
    title: string
    description: string
    imageUrl: string | null
    imageAlt: string
    artName: SiteContentCardArtName
    /** A draft that only the local site lists. Its title greys out so it reads apart from the real ones. */
    isUnpublished?: boolean
}

export const SiteContentCardGrid = ({ cards }: { cards: SiteContentCard[] }) => (
    <div className="site-content-grid">
        {cards.map((card) => (
            <SiteContentCardLink key={card.href} card={card} />
        ))}
    </div>
)

const SiteContentCardLink = ({ card }: { card: SiteContentCard }) => (
    <Link href={card.href} className="group site-content-card">
        <article>
            <div className="game-thumbnail">
                {card.imageUrl === null ? (
                    cardArtOfArtName[card.artName]
                ) : (
                    <Image
                        src={card.imageUrl}
                        alt={card.imageAlt}
                        fill={true}
                        sizes="(min-width: 1024px) 33vw, (min-width: 640px) 50vw, 100vw"
                        className="object-cover"
                        unoptimized={true}
                    />
                )}
                <div className="game-card-shade" />
            </div>

            <div>
                <h3 className={`line-clamp-2 ${card.isUnpublished === true ? 'text-gray-400' : 'text-foreground'}`}>{card.title}</h3>
                <p className="line-clamp-3">{card.description}</p>
            </div>
        </article>
    </Link>
)
