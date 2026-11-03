import type { Metadata } from 'next'
import { JsonLd } from '@/components/site/JsonLd'
import { SiteContentCardPage } from '@/components/site/SiteContentCardPage'
import { SiteContentCardGrid, type SiteContentCard } from '@/components/site/SiteContentCardGrid'
import { getBlogPosts } from '@/lib/blog'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'

export const dynamic = 'force-static'

export const metadata: Metadata = getSitePageMetadata({
    title: 'Blog',
    description: 'kube.new blog posts.',
    path: '/blog',
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

const BlogPage = async () => {
    const blogPosts = await getBlogPosts()
    // Priority 1 pins introductory posts, while the remaining posts form the development blog.
    const blogSections = [
        { title: 'Pinned', posts: blogPosts.filter((blogPost) => blogPost.priority === 1) },
        { title: 'Dev Blog', posts: blogPosts.filter((blogPost) => blogPost.priority !== 1) },
    ]

    return (
        <>
            <JsonLd value={getBreadcrumbJsonLd({ path: '/blog', pageName: 'Blog' })} />
            <SiteContentCardPage title="Blog" subtitle={undefined}>
                <div className="space-y-12">
                    {blogSections.map((section) => {
                        const blogCards: SiteContentCard[] = section.posts.map((blogPost) => ({
                            href: `/blog/${blogPost.slug}`,
                            title: blogPost.title,
                            description: blogPost.description,
                            imageUrl: blogPost.thumbnailImageUrl,
                            imageAlt: `${blogPost.title} splash image`,
                            artName: 'blog',
                            isUnpublished: !blogPost.isPublished,
                        }))

                        return (
                            <section key={section.title}>
                                <h2 className="site-section-title mb-5">{section.title}</h2>
                                <SiteContentCardGrid cards={blogCards} />
                            </section>
                        )
                    })}
                </div>
            </SiteContentCardPage>
        </>
    )
}

export default BlogPage
