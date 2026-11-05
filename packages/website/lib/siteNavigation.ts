import { BookOpen, Code, Gamepad2, Hammer, Shapes, UserRound, type LucideIcon } from 'lucide-react'

export type SiteNavigationItem = {
    href: string
    activePath: string
    label: SiteTopNavigationLabel
}

export type SiteTopNavigationLabel = 'Games' | 'Create' | 'Profile' | 'Catalog'
export type SiteResourceNavigationLabel = 'Assets' | 'Docs' | 'Blog'

export type SiteResourceNavigationItem = {
    href: string
    activePath: string
    label: SiteResourceNavigationLabel
}

export const CREATE_PATH = '/create'
// Every website entry to the editor opens the game's browser workspace.
export const EDITOR_BASE_PATH = '/editor'
export const EDITOR_PATH = (gameId: number) => `${EDITOR_BASE_PATH}/${gameId}`
export const BLOG_PATH = '/blog'
export const DOCS_PATH = '/docs'
export const GAMES_PATH = '/games'
export const ASSETS_PATH = '/assets'
export const CATALOG_PATH = '/catalog'
export const DATA_PATH = '/data'
export const TOP_GAMES_PATH = '/games/top'
export const RECENT_GAMES_PATH = '/games/recent'
export const PROFILE_PATH = '/profile'
export const DOWNLOAD_PATH = '/download'
export const PLANS_PATH = '/plans'
// Where Stripe sends the browser back to after checkout or the portal. See app/plans/return/route.ts.
export const PLANS_RETURN_PATH = `${PLANS_PATH}/return`
export const PRIVACY_PATH = '/privacy'
export const TERMS_PATH = '/terms'
export const DISCORD_URL = 'https://discord.gg/4uZsGRtGT'
export const GITHUB_URL = 'https://github.com/andrewpareles/kube'

// The header is fixed to the top of every page, so anything you jump to with a #anchor lands under it
// unless it keeps some room above itself. Put ANCHOR_SCROLL_MARGIN_CLASS_NAME on the element that the
// #anchor points at, or the descendant version on a container when the ids are on things inside it.
// Tailwind only picks up class names that are written out in full, which is why the descendant one
// repeats the distance instead of being built from the first one.
// Both forms use the responsive header offset from site.css, which the docs scroll spy also reads.
// Sending somebody to buy something always interrupts whatever they were doing, so the plans page
// carries where they were and the checkout returns them to it.
export const getPlansUrl = ({ next }: { next: string }) => `${PLANS_PATH}?next=${encodeURIComponent(next)}`

export const ANCHOR_SCROLL_MARGIN_CLASS_NAME = 'scroll-mt-[var(--site-anchor-offset)]'
export const DESCENDANT_ANCHOR_SCROLL_MARGIN_CLASS_NAME = '[&_[id]]:scroll-mt-[var(--site-anchor-offset)]'

// One entry for the whole games area, so it stays lit on a collection or on a game's own page
// rather than the header changing shape as you move around inside it.
export const siteTopNavigationItems: SiteNavigationItem[] = [
    { href: GAMES_PATH, activePath: '/games', label: 'Games' },
    { href: PROFILE_PATH, activePath: PROFILE_PATH, label: 'Profile' },
    { href: CATALOG_PATH, activePath: CATALOG_PATH, label: 'Catalog' },
    { href: CREATE_PATH, activePath: CREATE_PATH, label: 'Create' },
]

export const siteResourceNavigationItems: SiteResourceNavigationItem[] = [
    { href: ASSETS_PATH, activePath: ASSETS_PATH, label: 'Assets' },
    { href: DOCS_PATH, activePath: DOCS_PATH, label: 'Docs' },
    { href: BLOG_PATH, activePath: BLOG_PATH, label: 'Blog' },
]

export const siteHeaderTopNavigationItems = siteTopNavigationItems.filter(({ label }) => label === 'Games' || label === 'Create' || label === 'Catalog')
// Signed in, the resources move to the sidebar, but Assets also ends the header's links.
export const siteHeaderSignedInResourceNavigationItems = siteResourceNavigationItems.filter(({ label }) => label === 'Assets')
export const siteSidebarTopNavigationItems = siteTopNavigationItems.filter(({ label }) => label === 'Games' || label === 'Catalog')

// The destination can differ from the section's path, but its pages still highlight the same item.
// A section stays lit for everything underneath it, so a game's own page still shows Games. The
// slash matters: without it /gameshow would count as being inside /games.
export const isResourceNavigationItemActive = (pathname: string, { href, activePath }: { href: string, activePath: string }) =>
    pathname === href || pathname === activePath || pathname.startsWith(`${activePath}/`)

// The icons live here beside the labels rather than in the sidebar, because the sidebar is only
// one of the two places that draws this list. On a phone it is drawn as a menu in the header
// instead, and the two would drift apart if each kept its own icons.
// The Catalog's hat is HatIcon, which the sidebar draws itself, since the tests load this file and cannot load a .tsx one.
export const iconOfTopLabel: { [label in Exclude<SiteTopNavigationLabel, 'Catalog'>]: LucideIcon } = {
    'Games': Gamepad2,
    'Create': Hammer,
    'Profile': UserRound,
}

export const iconOfResourceLabel: { [label in SiteResourceNavigationLabel]: LucideIcon } = {
    'Assets': Shapes,
    Docs: Code,
    // The docs moved into the blog, and the book came with them.
    Blog: BookOpen,
}
