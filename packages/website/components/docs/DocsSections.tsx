import Link from 'next/link'
import type { ReactNode } from 'react'
import { DocsScrollSpyNav, type DocsScrollSpyNavSection } from '@/components/docs/DocsScrollSpyNav'
import { ANCHOR_SCROLL_MARGIN_CLASS_NAME } from '@/lib/siteNavigation'

// A docs page describes itself as one list of sections, and this file renders both the left navigation and the
// main content from that single list. Because a page only ever writes each name once, the name in the navigation
// is always the name of the heading it jumps to.
// The mobile contents links use Next's history just like the desktop navigation.

export type DocsSectionEntry = {
    name: string
    anchorId: string
    // Indenting and muting only change how the entry looks in the left navigation.
    isIndented: boolean
    isMuted: boolean
    content: ReactNode
}

export type DocsSection = {
    name: string
    isMuted: boolean
    entryHeadingSize: 'large' | 'small'
    // A section can show content of its own right under its heading, which is all a section like a system prompt needs.
    content: ReactNode | undefined
    entries: DocsSectionEntry[]
}

// A section heading is linked to by its name, so a page never writes an id for it either.
const anchorIdOfSectionName = (name: string): string => name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

export const DocsSectionsPage = ({ title, description, sections }: { title: string; description: ReactNode; sections: DocsSection[] }) => {
    const shownSections = sections.filter((section) => section.entries.length > 0 || section.content !== undefined)
    const navSections: DocsScrollSpyNavSection[] = shownSections.map((section) => ({
        name: section.name,
        anchorId: anchorIdOfSectionName(section.name),
        isMuted: section.isMuted,
        entries: section.entries.map((entry) => ({ name: entry.name, anchorId: entry.anchorId, isIndented: entry.isIndented, isMuted: entry.isMuted })),
    }))

    return (
        <div>
            <main className="site-container site-page">
                <div className="site-docs-layout">
                    <DocsScrollSpyNav sections={navSections} />
                    <details className="site-docs-mobile-nav lg:hidden">
                        <summary className="cursor-pointer">On This Page</summary>
                        <nav aria-label="Page sections">
                            {navSections.map((section) => (
                                <div key={section.anchorId} className="mb-4">
                                    <Link href={`#${section.anchorId}`} className="block py-1 font-semibold">{section.name}</Link>
                                    {section.entries.map((entry) => <Link key={entry.anchorId} href={`#${entry.anchorId}`} className="block py-1 pl-3 text-muted-foreground">{entry.name}</Link>)}
                                </div>
                            ))}
                        </nav>
                    </details>
                    <div className="min-w-0">
                        <h1 className="site-page-title mb-3">{title}</h1>
                        <div className="mb-10 max-w-3xl space-y-3 text-sm leading-6 text-gray-600">
                            {description}
                        </div>

                        <div className="space-y-16">
                            {shownSections.map((section) => (
                                <DocsSectionContent key={section.name} section={section} />
                            ))}
                        </div>
                    </div>
                </div>
            </main>
        </div>
    )
}

const DocsSectionContent = ({ section }: { section: DocsSection }) => (
    <div className="site-docs-section">
        <h2 id={anchorIdOfSectionName(section.name)} className={`site-section-title mb-6 ${ANCHOR_SCROLL_MARGIN_CLASS_NAME}`}>
            {section.name}
        </h2>
        {section.content}
        <div className={section.entryHeadingSize === 'large' ? 'space-y-16' : 'space-y-10'}>
            {section.entries.map((entry) => (
                <section key={entry.anchorId} id={entry.anchorId} className={`site-docs-entry ${ANCHOR_SCROLL_MARGIN_CLASS_NAME}`}>
                    <h3 className={section.entryHeadingSize === 'large' ? 'text-2xl font-semibold' : 'text-xl font-semibold'}>
                        {entry.name}
                    </h3>
                    {entry.content}
                </section>
            ))}
        </div>
    </div>
)
