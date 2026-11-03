import { readdir } from 'node:fs/promises'
import path from 'node:path'
import { getRequiredMarkdownMetadataValue, getWebsiteFilePath, readMarkdownFile } from './markdown.ts'

export type BlogPost = {
    slug: string
    /** An unpublished post is one being written: it shows locally and is left out of the built site. */
    isPublished: boolean
    title: string
    description: string
    date: string
    /** Lower sorts earlier, ahead of the date. 1 pins a post to the top of the listing; 5 is the ordinary case. */
    priority: number
    /** Who wrote it, as their public profile id. Their @username comes from that profile. */
    authorUserId: number
    authorName: string
    thumbnailImageUrl: string
    markdown: string
}

const BLOG_CONTENT_DIR = getWebsiteFilePath('app', 'blog', '[slug]')
const MARKDOWN_FILE_EXTENSION = '.md'
const shouldShowUnpublishedPosts = process.env.NODE_ENV === 'development'

// Blog posts are plain markdown files with a small metadata block so a new post is one file.
// Reading every post to find out which ones are published is what the listing does anyway, and
// there are few enough of them that the slug list is worth taking from the same place rather than
// working the names out separately and having two ideas of which posts there are.
export const getBlogPostSlugs = async () => (await getBlogPosts()).map((blogPost) => blogPost.slug).sort()

export const getBlogPosts = async () => {
    const fileNames = await readdir(BLOG_CONTENT_DIR)
    const slugs = fileNames
        .filter((fileName) => fileName.endsWith(MARKDOWN_FILE_EXTENSION))
        .map((fileName) => fileName.slice(0, -MARKDOWN_FILE_EXTENSION.length))
    const blogPosts = await Promise.all(slugs.map(readBlogPost))

    return blogPosts
        .filter((blogPost) => blogPost.isPublished || shouldShowUnpublishedPosts)
        .sort((blogPostA, blogPostB) => blogPostA.priority - blogPostB.priority || blogPostB.date.localeCompare(blogPostA.date))
}

export const getBlogPostBySlug = async (slug: string) => {
    const blogPosts = await getBlogPosts()
    return blogPosts.find((blogPost) => blogPost.slug === slug) ?? null
}

export const formatBlogPostDate = (date: string) => {
    const [year, month, day] = date.split('-').map(Number)
    return new Intl.DateTimeFormat('en-US', {
        month: 'long',
        day: 'numeric',
        year: 'numeric',
    }).format(new Date(year, month - 1, day))
}

const readBlogPost = async (slug: string): Promise<BlogPost> => {
    const markdownFilePath = path.join(BLOG_CONTENT_DIR, `${slug}${MARKDOWN_FILE_EXTENSION}`)
    const markdownFile = await readMarkdownFile(markdownFilePath)
    const getMetadataValue = (metadataKey: string) => getRequiredMarkdownMetadataValue({ ...markdownFile, metadataKey, markdownFilePath })

    const authorUserId = Number(getMetadataValue('author_id'))
    if (!Number.isInteger(authorUserId)) {
        throw new Error(`${markdownFilePath} needs a public profile id for its author, not a name.`)
    }

    const priority = Number(getMetadataValue('priority'))
    if (!Number.isInteger(priority)) {
        throw new Error(`${markdownFilePath} needs a whole number for priority. Use 5 unless the post should be pinned above the others.`)
    }

    return {
        slug,
        isPublished: getMetadataValue('isPublished') === 'true',
        title: getMetadataValue('title'),
        description: getMetadataValue('description'),
        date: getMetadataValue('date'),
        priority,
        authorUserId,
        authorName: getMetadataValue('author_name'),
        thumbnailImageUrl: getMetadataValue('thumbnail_image_url'),
        markdown: markdownFile.markdown,
    }
}
