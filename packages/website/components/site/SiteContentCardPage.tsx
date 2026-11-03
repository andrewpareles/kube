import type { ReactNode } from 'react'

// Docs, API, and Blog share this shell so their titles and card layouts stay the same. The
// subtitle is a line under the title for a page that has something to say before its cards; a page
// with nothing to add passes undefined and the gap below the title stays exactly as it was.
// Each page supplies its grids so it can group cards under section headings when needed.
export const SiteContentCardPage = ({ title, subtitle, children }: { title: string; subtitle: string | undefined; children: ReactNode }) => (
    <div>
        <main className="site-container site-page">
            <div className="mb-10">
                <h1 className="site-page-title">{title}</h1>
                {subtitle !== undefined && <p className="site-page-description">{subtitle}</p>}
            </div>
            {children}
        </main>
    </div>
)
