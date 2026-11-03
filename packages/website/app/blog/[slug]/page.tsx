import Link from 'next/link'
import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { cache } from 'react'
import { RenderMarkdown } from '@/components/site/RenderMarkdown'
import { JsonLd } from '@/components/site/JsonLd'
import { SiteBackLink } from '@/components/site/SiteBackLink'
import { formatBlogPostDate, getBlogPostBySlug, getBlogPostSlugs } from '@/lib/blog'
import { getProfileUsername } from '@/lib/siteProfiles'
import { BLOG_PATH } from '@/lib/siteNavigation'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { ORGANIZATION_NAME, getSiteArticleMetadata, getSiteUrl } from '@/lib/siteMetadata'

export const dynamicParams = false

export const generateStaticParams = async () => {
    const slugs = await getBlogPostSlugs()
    return slugs.map((slug) => ({ slug }))
}
const getBlogPostCached = cache(async (slug: string) => {
    const blogPost = await getBlogPostBySlug(slug)
    if (blogPost === null) notFound()
    return blogPost
})

const getBlogPostFromParams = async (params: Promise<{ slug: string }>) => {
    const { slug } = await params
    return getBlogPostCached(slug)
}

// metadata (dynamic per blog)
export const generateMetadata = async ({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> => {
    const blogPost = await getBlogPostFromParams(params)
    return getSiteArticleMetadata({
        title: blogPost.title,
        description: blogPost.description,
        path: `${BLOG_PATH}/${blogPost.slug}`,
        imageUrl: blogPost.thumbnailImageUrl,
        imageAlt: `${blogPost.title} article thumbnail`,
        publishedTime: blogPost.date,
    })
}

const BlogPostPage = async ({
    params,
}: {
    params: Promise<{ slug: string }>
}) => {
    const blogPost = await getBlogPostFromParams(params)
    const authorUsername = await getProfileUsername(blogPost.authorUserId)
    const blogPostPath = `${BLOG_PATH}/${blogPost.slug}`

    return (
        <>
            <JsonLd value={[
                {
                    '@context': 'https://schema.org',
                    '@type': 'BlogPosting',
                    headline: blogPost.title,
                    description: blogPost.description,
                    image: getSiteUrl(blogPost.thumbnailImageUrl),
                    datePublished: blogPost.date,
                    mainEntityOfPage: getSiteUrl(blogPostPath),
                    author: {
                        '@type': 'Person',
                        name: blogPost.authorName,
                        url: authorUsername === null ? undefined : getSiteUrl(`/profile/${blogPost.authorUserId}`),
                    },
                    publisher: {
                        '@type': 'Organization',
                        name: ORGANIZATION_NAME,
                        url: getSiteUrl('/'),
                        logo: {
                            '@type': 'ImageObject',
                            url: getSiteUrl('/icon.png'),
                        },
                    },
                },
                getBreadcrumbJsonLd({ path: blogPostPath, pageName: blogPost.title }),
            ]} />
            <div>
                <main className="site-container site-page site-prose-page">
                    <article>
                        <SiteBackLink href={BLOG_PATH} label="Blog" />
                        <h1 className="site-article-title mb-8">{blogPost.title}</h1>
                        <RenderMarkdown markdown={blogPost.markdown} />
                        {/* The author closes the post, with the date pushed out to the right edge of the same
                            line. They wrap onto separate lines when the byline is too long to share one, and the
                            date keeps to the right either way. */}
                        <div className="mt-10 flex flex-wrap items-baseline gap-x-4 gap-y-1 border-t border-border pt-6 text-sm text-muted-foreground">
                            {/* The @username comes from the author's own profile rather than from the post, so
                                renaming an account renames them everywhere at once. The name beside it is the
                                post's own, which is what still names the author once an account is gone. */}
                            <p>
                                {blogPost.authorName}
                                {authorUsername !== null && (
                                    <>
                                        {' '}
                                        {'('}
                                        <Link
                                            href={`/profile/${blogPost.authorUserId}`}
                                            className="transition-colors hover:text-gray-600"
                                        >
                                            @{authorUsername}
                                        </Link>
                                        {')'}
                                    </>
                                )}
                            </p>
                            <time className="ml-auto" dateTime={blogPost.date}>{formatBlogPostDate(blogPost.date)}</time>
                        </div>
                    </article>
                </main>
            </div>
        </>
    )
}

export default BlogPostPage
