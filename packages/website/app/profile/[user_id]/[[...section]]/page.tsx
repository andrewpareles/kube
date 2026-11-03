import type { Metadata } from 'next'
import Link from 'next/link'
import { notFound } from 'next/navigation'
import { cache, type ComponentType, type ReactNode } from 'react'
import { ArrowRight, CircleUserRound } from 'lucide-react'
import { AssetCard } from '@/components/site/AssetCard'
import { AvatarDraftProvider } from '@/components/site/AvatarDraft'
import { AvatarEditor } from '@/components/site/AvatarEditor'
import { CatalogItemCard } from '@/components/site/CatalogItemCard'
import { GameCardPlay } from '@/components/site/GameCardPlay'
import { HatIcon } from '@/components/site/HatIcon'
import { JsonLd } from '@/components/site/JsonLd'
import { NoHatsYet } from '@/components/site/NoHatsYet'
import { PlayerAvatar } from '@/components/site/PlayerAvatar'
import { SitePagination } from '@/components/site/SitePagination'
import { getMyGamesSplitByPublishState } from '@/lib/myGamesSections'
import { ASSETS_PAGE_SIZE, countProfileAssets, getProfileAssets } from '@/lib/siteAssets'
import { getPlayerLook, type PlayerLook } from '@/lib/siteAvatar'
import { getProfileCatalogItems, getProfileOwnedCatalogItems } from '@/lib/siteCatalog'
import { toGameCardData } from '@/lib/siteGameCards'
import { getAllowedProfileGames, type SiteGameSummary } from '@/lib/siteGames'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { CREATE_PATH, iconOfResourceLabel, iconOfTopLabel } from '@/lib/siteNavigation'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata, getSiteUrl } from '@/lib/siteMetadata'
import { ChangeUsernameBanner } from '../ChangeUsernameBanner'
import { ProfileMenu } from '../ProfileMenu'

const getUserIdFromParams = async (params: Promise<{ user_id: string }>) => {
    const { user_id: userIdString } = await params
    const userId = Number(userIdString)
    if (!Number.isInteger(userId)) notFound()
    return userId
}

const getAllowedProfileGamesCached = cache(async (userId: number) => {
    const allowedProfileGames = await getAllowedProfileGames(userId)
    if (allowedProfileGames === null) notFound()
    return allowedProfileGames
})

// metadata (dynamic per profile)
export const generateMetadata = async ({ params }: { params: Promise<{ user_id: string }> }): Promise<Metadata> => {
    const userId = await getUserIdFromParams(params)
    const allowedProfileGames = await getAllowedProfileGamesCached(userId)
    const profileName = allowedProfileGames.username
    const profilePath = `/profile/${userId}`
    const shouldIndexProfile = allowedProfileGames.games.some((game) => game.isPublished)
    const metadata = getSitePageMetadata({
        title: profileName,
        description: `Play games made by ${profileName} on kube.new.`,
        path: profilePath,
        imageUrl: SITE_DEFAULT_IMAGE_PATH,
        imageAlt: SITE_DEFAULT_IMAGE_ALT,
    })

    return shouldIndexProfile ? metadata : {
        ...metadata,
        robots: { index: false, follow: true },
    }
}

type ProfileSectionKey = 'avatar' | 'games' | 'hats' | 'assets'

// The sections at the top of every profile. Hats is the items the person owns, then the items they made, if any.
// Assets only shows when the person made any. Avatar only shows on your own profile.
// Games and Assets take the same icons as in the site's sidebar. On your own profile, the items you made are called
// My Listings.
const profileSections: { key: ProfileSectionKey, label: string, Icon: ComponentType<{ className: string }> }[] = [
    { key: 'games', label: 'Games', Icon: iconOfTopLabel['Games'] },
    { key: 'avatar', label: 'Avatar', Icon: CircleUserRound },
    { key: 'hats', label: 'Hats', Icon: HatIcon },
    { key: 'assets', label: 'Assets', Icon: iconOfResourceLabel['Assets'] },
]

const ProfileCardGroup = ({ title, subtitle, cards, empty = <p className="text-sm text-gray-500">None yet.</p> }: { title: string, subtitle?: ReactNode, cards: ReactNode[], empty?: ReactNode }) => (
    <section>
        <div className="mb-4">
            <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
            {subtitle}
        </div>
        {cards.length === 0 ? empty : <div className="site-game-grid">{cards}</div>}
    </section>
)

// Only the section on show is read from the database. Games splits into public and private, and only the owner sees
// the private ones, and only when there are any. Hats lists the items the person made under the ones they own, when there are any.
const getProfileSectionContent = async ({ sectionKey, title, userId, isOwner, look, publishedGames, privateGames, assetsPage, assetCount }: { sectionKey: ProfileSectionKey, title: string, userId: number, isOwner: boolean, look: PlayerLook, publishedGames: SiteGameSummary[], privateGames: SiteGameSummary[], assetsPage: number, assetCount: number }): Promise<ReactNode> => {
    switch (sectionKey) {
        case 'avatar': return <AvatarEditor look={look} hats={await getProfileOwnedCatalogItems(userId)} />
        case 'games': return <div className="space-y-16">
            <ProfileCardGroup title={title} cards={publishedGames.map((game) => <GameCardPlay key={game.gameId} game={toGameCardData(game)} playerStat="live" />)} empty={<p className="text-sm text-gray-500">
                No games yet.{isOwner && privateGames.length === 0 && <> <Link href={CREATE_PATH} className="font-medium text-kube-blue hover:underline">Make a Game.</Link></>}
            </p>} />
            {isOwner && privateGames.length > 0 && <>
                <hr className="border-gray-200" />
                <ProfileCardGroup title="Private Games" cards={privateGames.map((game) => <GameCardPlay key={game.gameId} game={toGameCardData(game)} playerStat="live" />)} />
            </>}
        </div>
        case 'hats': {
            const [ownedItems, madeItems] = await Promise.all([getProfileOwnedCatalogItems(userId), getProfileCatalogItems({ userId, isOwner })])
            return <div className="space-y-16">
                <ProfileCardGroup title={title} subtitle={isOwner && ownedItems.length > 0 && <Link href={`/profile/${userId}/avatar`} scroll={false} className="mt-1 inline-flex items-center gap-1 text-sm font-medium text-kube-blue hover:underline">Change My Hat <ArrowRight className="size-3.5" aria-hidden="true" /></Link>} cards={ownedItems.map((item) => <CatalogItemCard key={item.catalogItemId} item={item} isShopInfoShown={false} showOwnedCheckmark={false} />)} empty={isOwner ? <NoHatsYet /> : undefined} />
                {madeItems.length > 0 && <>
                    <hr className="border-gray-200" />
                    <ProfileCardGroup title={isOwner ? 'My Listings' : 'Catalog'} cards={madeItems.map((item) => <CatalogItemCard key={item.catalogItemId} item={item} isShopInfoShown={true} showOwnedCheckmark={false} />)} />
                </>}
            </div>
        }
        case 'assets': return <>
            <ProfileCardGroup title={title} cards={(await getProfileAssets({ userId, offset: (assetsPage - 1) * ASSETS_PAGE_SIZE, limit: ASSETS_PAGE_SIZE })).map((asset) => <AssetCard key={asset.assetId} asset={asset} />)} />
            <SitePagination page={assetsPage} pageCount={Math.ceil(assetCount / ASSETS_PAGE_SIZE)} hrefOfPage={(nextPage) => `/profile/${userId}/assets?page=${nextPage}`} label="Assets pages" className="mt-8" />
        </>
    }
}

export default async function ProfilePage({ params, searchParams }: { params: Promise<{ user_id: string, section?: string[] }>, searchParams: Promise<{ page?: string }> }) {
    const userId = await getUserIdFromParams(params)
    const { section: sectionSegments } = await params
    const { page: pageQuery } = await searchParams
    const requestedPage = Number(pageQuery ?? '1')
    const assetsPage = Number.isSafeInteger(requestedPage) && requestedPage > 0 ? requestedPage : 1
    const allowedProfileGames = await getAllowedProfileGamesCached(userId)
    const { isOwner, username, createdAt, games } = allowedProfileGames
    const [assetCount, look] = await Promise.all([countProfileAssets({ userId }), getPlayerLook(userId)])
    const visibleSections = profileSections.filter(({ key }) => (key !== 'avatar' || isOwner) && (key !== 'assets' || assetCount > 0))
    const sectionPath = sectionSegments?.join('/') ?? visibleSections[0].key
    const section = visibleSections.find(({ key }) => key === sectionPath)
    if (section === undefined) notFound()
    const { publishedGames, privateGames } = getMyGamesSplitByPublishState(games)
    const sectionContent = await getProfileSectionContent({ sectionKey: section.key, title: section.label, userId, isOwner, look, publishedGames, privateGames, assetsPage, assetCount })
    const profilePath = `/profile/${userId}`

    return (
        <>
            {publishedGames.length === 0 ? null : (
                <JsonLd value={[
                    {
                        '@context': 'https://schema.org',
                        '@type': 'ProfilePage',
                        url: getSiteUrl(profilePath),
                        dateCreated: createdAt ?? undefined,
                        mainEntity: {
                            '@type': 'Person',
                            name: username,
                            identifier: userId,
                            url: getSiteUrl(profilePath),
                        },
                        hasPart: publishedGames.map((game) => ({
                            '@type': 'VideoGame',
                            name: game.name,
                            url: getSiteUrl(`/games/${game.gameId}`),
                        })),
                    },
                    getBreadcrumbJsonLd({ path: profilePath, pageName: username }),
                ]} />
            )}
            <AvatarDraftProvider>
                <main className="site-container site-page site-full-width-page">
                    {isOwner && username === `Guest-${userId}` && <div className="mb-8"><ChangeUsernameBanner look={look} /></div>}
                    {/* TODO: AI-PICKED-VALUE: A 120px circle for the avatar, 20px from the name. */}
                    <div className="mb-10 flex items-center gap-5">
                        <PlayerAvatar look={look} isProfileAvatar={true} className="size-30" />
                        <h1 className="site-page-title min-w-0 break-words">{username}</h1>
                        {isOwner && <div className="-ml-3 translate-y-0.75"><ProfileMenu /></div>}
                    </div>
                    {/* The sections run in one row above the cards, and scroll sideways when the screen is too narrow for them. The row
                        is pulled out by each tab's own 12px, so the first tab's highlight lines up with the cards. */}
                    <div className="flex flex-col gap-6">
                        <nav aria-label="Profile sections" className="-mx-3 flex overflow-x-auto">
                            {visibleSections.map(({ key, label, Icon }) => (
                                <Link key={key} href={`${profilePath}/${key}`} scroll={false} aria-current={key === section.key ? 'page' : undefined} className="site-sidebar-link shrink-0">
                                    <span className="site-sidebar-link-inner whitespace-nowrap"><Icon className="size-5" />{label}</span>
                                </Link>
                            ))}
                        </nav>
                        <div className="min-w-0 flex-1">{sectionContent}</div>
                    </div>
                </main>
            </AvatarDraftProvider>
        </>
    )
}
