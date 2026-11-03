import type { Metadata } from 'next'
import { Download } from 'lucide-react'
import { Button } from '@/shadcn/components/ui/button'
import { JsonLd } from '@/components/site/JsonLd'
import { SiteBackdropCard } from '@/components/site/SiteBackdropCard'
import { CLIENT_DOWNLOADS, DownloadPageRow, getDownloadUrlForOS } from '@/lib/clientDownloadUrls'
import { getBackdropGames } from '@/lib/siteGames'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, SITE_DESCRIPTION, SITE_NAME, getSitePageMetadata, getSiteUrl } from '@/lib/siteMetadata'


const DOWNLOAD_PAGE_ROWS: DownloadPageRow[] = [
    {
        label: 'Mobile',
        href: null,
    },
    {
        label: 'macOS (Apple Silicon)',
        href: getDownloadUrlForOS({ platform: 'macos', architecture: 'arm64' })?.href ?? null,
    },
    {
        label: 'Windows (x64)',
        href: getDownloadUrlForOS({ platform: 'windows', architecture: 'x64' })?.href ?? null,
    },
    {
        label: 'macOS (Intel)',
        href: null,
    },
    {
        label: 'Windows (Arm)',
        href: null,
    },
]

export const metadata: Metadata = getSitePageMetadata({
    title: 'Download kube.new',
    description: 'Download the kube.new desktop app.',
    path: '/download',
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

const downloadActionClassName = 'h-8 w-full rounded-xl px-3 text-xs font-semibold whitespace-nowrap shadow-none sm:text-sm'
const downloadRowClassName = 'grid grid-cols-[minmax(0,8.5rem)_6.5rem] items-center justify-center gap-3 sm:grid-cols-[10rem_7.5rem] sm:gap-4'

export default async function DownloadPage() {
    return (
        <>
            <JsonLd value={[
                {
                    '@context': 'https://schema.org',
                    '@type': 'SoftwareApplication',
                    name: SITE_NAME,
                    description: SITE_DESCRIPTION,
                    url: getSiteUrl('/download'),
                    image: getSiteUrl(SITE_DEFAULT_IMAGE_PATH),
                    operatingSystem: ['macOS', 'Windows'],
                    downloadUrl: CLIENT_DOWNLOADS.map((clientDownload) => clientDownload.href),
                },
                getBreadcrumbJsonLd({ path: '/download', pageName: 'Download' }),
            ]} />
            <SiteBackdropCard backgroundGames={await getBackdropGames()}>
                <h1 className="site-page-title text-center mb-10">All Downloads</h1>

                <div className="flex min-w-0 flex-col gap-3">
                    {DOWNLOAD_PAGE_ROWS.map((downloadRow) => {
                        return (
                            <div key={downloadRow.label} className={downloadRowClassName}>
                                <div className="min-w-0 text-xs font-medium leading-5 text-gray-700 sm:text-sm">
                                    {downloadRow.label}
                                </div>

                                {downloadRow.href === null ? (
                                    <Button type="button" variant="secondary" size="sm" disabled={true} className={`${downloadActionClassName} disabled:opacity-100`}>
                                        Coming soon
                                    </Button>
                                ) : (
                                    <Button asChild variant="default" size="sm" className={downloadActionClassName}>
                                        <a href={downloadRow.href}>
                                            <Download />
                                            Download
                                        </a>
                                    </Button>
                                )}
                            </div>
                        )
                    })}
                </div>
            </SiteBackdropCard>
        </>
    )
}
