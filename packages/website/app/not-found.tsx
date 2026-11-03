import Link from 'next/link'
import type { Metadata } from 'next'
import { GAMES_PATH } from '@/lib/siteNavigation'
import { getNoIndexPageMetadata } from '@/lib/siteMetadata'
import { NotFoundKubeMark } from './NotFoundKubeMark'

export const metadata: Metadata = getNoIndexPageMetadata({
    title: 'Page Not Found',
    description: null,
})

// Next renders this for anything that has no route and for any page that calls notFound(). It
// sits inside the usual header and sidebar, so a wrong address still leaves you somewhere.
const NotFoundPage = () => (
    <div className="site-backdrop-page">
        <main className="flex w-full max-w-xl flex-col items-center py-12 text-center">
            <NotFoundKubeMark sidePx={160} renderId="not-found" />

            <h1 className="site-page-title">Page Not Found</h1>
            <p className="mt-3 max-w-md text-balance text-gray-500">
                This page doesn’t exist.{' '}
                <Link href={GAMES_PATH} className="text-gray-950 underline underline-offset-4 hover:text-gray-600">Explore Games</Link>.
            </p>

        </main>
    </div>
)

export default NotFoundPage
