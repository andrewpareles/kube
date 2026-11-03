'use client'

import { useEffect, useRef, type ReactNode } from 'react'
import { usePathname } from 'next/navigation'
import { SiteHeader } from '@/components/site/SiteHeader'
import { useUser } from '@/components/context/useUser'
import { DOWNLOAD_PATH, EDITOR_BASE_PATH } from '@/lib/siteNavigation'
import { SiteBrandLink } from '@/components/site/SiteBrandLink'
import { SiteSidebar } from './SiteSidebar'
import { GoogleOneTap } from './login/GoogleOneTap'

type SiteLayoutShellProps = {
    children: ReactNode
    footer: ReactNode
}

export const SiteLayoutShell = ({ children, footer }: SiteLayoutShellProps) => {
    const pathname = usePathname()
    const user = useUser()
    const shownPathnameRef = useRef(pathname)
    const isHistoryNavigationRef = useRef(false)

    useEffect(() => {
        const onPopState = () => { isHistoryNavigationRef.current = true }
        window.addEventListener('popstate', onPopState)
        return () => window.removeEventListener('popstate', onPopState)
    }, [])

    // Next only scrolls a new page into view when its top is off screen, so a page opened from a little way down kept
    // that scroll. Back and forward are left to the browser, which puts the page back where it was.
    useEffect(() => {
        if (shownPathnameRef.current === pathname) return
        shownPathnameRef.current = pathname
        if (isHistoryNavigationRef.current) isHistoryNavigationRef.current = false
        else window.scrollTo({ top: 0, behavior: 'instant' })
    }, [pathname])

    // The editor owns the full viewport and provides its own navigation back to the game.
    if (pathname.startsWith(`${EDITOR_BASE_PATH}/`)) return children

    return (
        <div className="site-shell">
            <div className="site-columns">
                {user !== null && pathname !== DOWNLOAD_PATH && !pathname.startsWith('/auth/') && <SiteSidebar brand={<SiteBrandLink renderId="site-sidebar" logoSidePx={56} />} />}
                <div className="site-header-and-body">
                    <SiteHeader />
                    {/* The room set aside for the sidebar goes away with it on a phone, so we add padding left. */}
                    {/* Navigation now lives in the header at every width; all pages share the same edges. */}
                    <div className="site-body">
                        <div className="site-main">
                            {children}
                        </div>
                        {footer}
                    </div>
                </div>
            </div>
            {user === null && <GoogleOneTap />}
        </div>
    )
}
