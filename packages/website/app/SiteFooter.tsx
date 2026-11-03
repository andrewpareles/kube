import Link from 'next/link'
import { DISCORD_URL } from '@/lib/siteNavigation'

export const SiteFooter = () => (
    <footer className="site-footer">
        <div>
            <nav aria-label="Footer" className="flex flex-wrap items-center justify-center gap-6">
                <Link href="/docs" className="transition-colors hover:text-gray-950">
                    Docs
                </Link>
                <Link href="/blog" className="transition-colors hover:text-gray-950">
                    Blog
                </Link>
                <a href={DISCORD_URL} target="_blank" rel="noreferrer" className="transition-colors hover:text-gray-950">
                    Discord
                </a>
                <Link href="/privacy" className="transition-colors hover:text-gray-950">
                    Privacy
                </Link>
                <Link href="/terms" className="transition-colors hover:text-gray-950">
                    Terms
                </Link>
            </nav>
        </div>
    </footer>
)
