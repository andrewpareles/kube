'use client'

import Link from 'next/link'
import { useEffect, useRef, useState } from 'react'
import { ScrollArea } from '@/shadcn/components/ui/scroll-area'

export type DocsScrollSpyNavEntry = {
    name: string
    anchorId: string
    isIndented: boolean
    isMuted: boolean
}

export type DocsScrollSpyNavSection = {
    name: string
    anchorId: string
    isMuted: boolean
    entries: DocsScrollSpyNavEntry[]
}

// The names shown here always come from DocsSectionsPage, which renders the same names as the headings in the main content.
// Section jumps use Next's history so Back restores the docs after visiting another page.
export const DocsScrollSpyNav = ({ sections }: { sections: DocsScrollSpyNavSection[] }) => {
    const ids = linkIdsOfSections(sections)
    const idsKey = ids.join('\n')
    const indicatorContentRef = useRef<HTMLDivElement>(null)
    const [activeId, setActiveId] = useState(ids[0] ?? '')
    const [indicatorTop, setIndicatorTop] = useState<number | null>(null)

    useEffect(() => {
        const updateActiveId = () => {
            let nextActiveId = ids[0] ?? ''

            // The first heading showing in the top half of the screen is active. If none is, the last heading scrolled up under the header is.
            for (const id of ids) {
                const element = document.getElementById(id)
                if (element === null) continue

                const elementTop = element.getBoundingClientRect().top
                if (elementTop > window.innerHeight / 2) break
                nextActiveId = id
                const scrollMarginTop = Number.parseFloat(window.getComputedStyle(element).scrollMarginTop)
                if (elementTop >= scrollMarginTop - 1) break
            }

            setActiveId((previousActiveId) => previousActiveId === nextActiveId ? previousActiveId : nextActiveId)
        }

        updateActiveId()
        window.addEventListener('scroll', updateActiveId, { passive: true })
        window.addEventListener('resize', updateActiveId)
        return () => {
            window.removeEventListener('scroll', updateActiveId)
            window.removeEventListener('resize', updateActiveId)
        }
    }, [idsKey])

    useEffect(() => {
        const updateIndicatorTop = () => {
            const indicatorContent = indicatorContentRef.current
            if (indicatorContent === null || activeId === '') {
                setIndicatorTop(null)
                return
            }

            const activeLink = indicatorContent.querySelector('[data-scroll-spy-current="true"]')
            if (!(activeLink instanceof HTMLElement)) {
                setIndicatorTop(null)
                return
            }

            const indicatorContentBounds = indicatorContent.getBoundingClientRect()
            const activeLinkBounds = activeLink.getBoundingClientRect()
            setIndicatorTop(activeLinkBounds.top - indicatorContentBounds.top + activeLinkBounds.height / 2)
        }

        updateIndicatorTop()
        window.addEventListener('resize', updateIndicatorTop)
        return () => {
            window.removeEventListener('resize', updateIndicatorTop)
        }
    }, [activeId, idsKey])

    // When the current section changes and its link is out of view in the overview, the overview scrolls itself to center it.
    // Only the overview scrolls, never the page.
    useEffect(() => {
        const indicatorContent = indicatorContentRef.current
        const viewport = indicatorContent?.closest('[data-slot="scroll-area-viewport"]')
        const activeLink = indicatorContent?.querySelector('[data-scroll-spy-current="true"]')
        if (!(viewport instanceof HTMLElement) || !(activeLink instanceof HTMLElement)) return

        const viewportBounds = viewport.getBoundingClientRect()
        const activeLinkBounds = activeLink.getBoundingClientRect()
        const margin = 48
        if (activeLinkBounds.top >= viewportBounds.top + margin && activeLinkBounds.bottom <= viewportBounds.bottom - margin) return

        const activeLinkMiddle = activeLinkBounds.top - viewportBounds.top + viewport.scrollTop + activeLinkBounds.height / 2
        viewport.scrollTo({ top: activeLinkMiddle - viewport.clientHeight / 2, behavior: 'smooth' })
    }, [activeId])

    return (
        <nav aria-label="Page sections" className="hidden min-w-0 lg:block">
            <div className="site-docs-nav">
                <ScrollArea className="h-full">
                    <div ref={indicatorContentRef} className="relative ml-1 pr-2">
                        {indicatorTop !== null && <span aria-hidden="true" className="pointer-events-none absolute left-0 z-10 h-4 w-px -translate-y-1/2 rounded-full bg-kube-blue transition-[top] duration-200 ease-out" style={{ top: indicatorTop }} />}
	                        <div>
		                            {sections.map((section, sectionIndex) => {
		                                return (
		                                    <div key={section.name}>
		                                        <Link href={`#${section.anchorId}`} data-scroll-spy-id={section.anchorId} data-scroll-spy-current={section.anchorId === activeId ? 'true' : undefined} aria-current={section.anchorId === activeId ? 'location' : undefined} className={sectionTitleClassName(section, sectionIndex !== 0)}>
		                                            {section.name}
		                                        </Link>
	                                        <div className="flex flex-col">
                                            {section.entries.map((entry) => (
	                                                <Link key={entry.anchorId} href={`#${entry.anchorId}`} data-scroll-spy-id={entry.anchorId} data-scroll-spy-current={entry.anchorId === activeId ? 'true' : undefined} aria-current={entry.anchorId === activeId ? 'location' : undefined} className={entryLinkClassName(entry)}>
	                                                    <span aria-hidden="true" className="pointer-events-none absolute left-0 top-0 z-0 h-full w-px bg-gray-100" />
	                                                    {entry.name}
	                                                </Link>
                                            ))}
                                        </div>
                                    </div>
                                )
                            })}
                        </div>
                    </div>
                </ScrollArea>
            </div>
        </nav>
    )
}

const linkIdsOfSections = (sections: DocsScrollSpyNavSection[]): string[] => {
    const ids: string[] = []
    for (const section of sections) {
        ids.push(section.anchorId)
        for (const entry of section.entries) ids.push(entry.anchorId)
    }
    return ids
}

const sectionTitleClassName = (section: DocsScrollSpyNavSection, hasTopMargin: boolean): string => {
    const marginClassName = hasTopMargin ? 'mt-3 ' : ''
    return section.isMuted ? `${marginClassName}block pt-1 pb-0.5 pl-2 pr-2 text-xs font-semibold text-gray-400 hover:text-gray-700` : `${marginClassName}block pt-1 pb-0.5 pl-2 pr-2 text-xs font-semibold text-foreground`
}

const entryLinkClassName = (entry: DocsScrollSpyNavEntry): string => {
    const indentClassName = entry.isIndented ? 'pl-8 ' : 'pl-5 '
    if (entry.isMuted) return `relative block w-full py-0.5 pr-2 ${indentClassName}text-gray-400 hover:text-gray-700`
    return `relative block w-full py-0.5 pr-2 ${indentClassName}text-gray-500 hover:text-gray-950`
}
