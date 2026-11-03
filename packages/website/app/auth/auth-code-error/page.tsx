import Link from 'next/link'
import type { Metadata } from 'next'
import { getNoIndexPageMetadata } from '@/lib/siteMetadata'
import { getLoginUrl } from '@/app/deeplink/deeplinkFlowConsts'

export const metadata: Metadata = getNoIndexPageMetadata({
    title: 'Authentication Error',
    description: null,
})

// redirect to this page to show an error happened
export default async function Page({ searchParams }: { searchParams: Promise<{ error: string }> }) {
    const params = await searchParams

    return (
        <div className="site-backdrop-page flex-col">
            <Link href="/" className="text-sm text-gray-400 hover:text-gray-950 transition-colors mb-10">
                ← kube.new
            </Link>
            <div className="w-full max-w-sm text-center">
                <h1 className="site-page-title mb-3">Something Went Wrong</h1>
                <p className="text-gray-500 text-sm mb-8">
                    {params?.error ?? 'An unexpected error occurred. Please try again or reach out so we can help.'}
                </p>
                <Link
                    href={getLoginUrl({ next: '/' })}
                    className="inline-block px-6 py-2.5 bg-primary text-white text-sm font-medium rounded-xl hover:bg-primary/90 transition-colors"
                >
                    Back to Login
                </Link>
            </div>
        </div>
    )
}
