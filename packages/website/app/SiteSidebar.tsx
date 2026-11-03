'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import type { ReactNode } from 'react'
import { CircleUserRound, Hammer } from 'lucide-react'
import { usePlayerLook, usePlayerName, usePlayerUserId } from '@/components/context/useUser'
import { HatIcon } from '@/components/site/HatIcon'
import { PlayerAvatar } from '@/components/site/PlayerAvatar'
import { CREATE_PATH, PROFILE_PATH, iconOfResourceLabel, iconOfTopLabel, isResourceNavigationItemActive, siteResourceNavigationItems, siteSidebarTopNavigationItems } from '@/lib/siteNavigation'

const SiteSidebarLink = ({ href, label, icon, isCurrentPage, needsLogin = false }: { href: string, label: string, icon: ReactNode, isCurrentPage: boolean, needsLogin?: boolean }) => (
    <Link href={href} aria-current={isCurrentPage ? 'page' : undefined} className={needsLogin ? 'site-sidebar-link site-sidebar-needs-login-link' : 'site-sidebar-link'}>
        <span className="site-sidebar-link-inner">
            <span className="site-sidebar-link-icon">{icon}</span>
            {label}
        </span>
    </Link>
)

export const SiteSidebar = ({ brand }: { brand: ReactNode }) => {
    const pathname = usePathname()
    const playerName = usePlayerName()
    const playerUserId = usePlayerUserId()
    const look = usePlayerLook()
    const playerProfilePath = playerUserId === null ? null : `${PROFILE_PATH}/${playerUserId}`

    return (
        <nav aria-label="Sidebar" className="site-sidebar">
            <div className="site-sidebar-top">
                {brand !== null && <div className="site-sidebar-brand">{brand}</div>}
                {playerName !== null && playerProfilePath !== null
                    ? <SiteSidebarLink href={playerProfilePath} label="Profile" icon={look === null ? <CircleUserRound className="size-5" /> : <PlayerAvatar look={look} isProfileAvatar={false} className="size-6" />} isCurrentPage={pathname === playerProfilePath} />
                    : <SiteSidebarLink href={PROFILE_PATH} label="Profile" icon={<CircleUserRound className="size-5" />} isCurrentPage={false} needsLogin />}
                {siteSidebarTopNavigationItems.map((navigationItem) => {
                    const isHatCatalog = navigationItem.label === 'Catalog'
                    const Icon = navigationItem.label === 'Catalog' ? HatIcon : iconOfTopLabel[navigationItem.label]
                    return <SiteSidebarLink key={navigationItem.label} href={navigationItem.href} label={isHatCatalog ? 'Hats' : navigationItem.label} icon={<Icon className="size-5" />} isCurrentPage={isResourceNavigationItemActive(pathname, navigationItem)} />
                })}
                <SiteSidebarLink href={CREATE_PATH} label="Create" icon={<Hammer className="size-5" />} isCurrentPage={pathname === CREATE_PATH} />
            </div>
            <div className="site-sidebar-bottom">
                {siteResourceNavigationItems.map((navigationItem) => {
                    const Icon = iconOfResourceLabel[navigationItem.label]
                    return <SiteSidebarLink key={navigationItem.label} href={navigationItem.href} label={navigationItem.label} icon={<Icon className="size-5" />} isCurrentPage={isResourceNavigationItemActive(pathname, navigationItem)} />
                })}
            </div>
        </nav>
    )
}
