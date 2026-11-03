import { RenderMarkdown } from './RenderMarkdown'

// A page that is nothing but one markdown document, which is what the privacy policy and the terms
// are. The title comes from the markdown file's frontmatter, so each document names itself once.
export const SiteMarkdownPage = ({ title, markdown }: { title: string; markdown: string }) => (
    <div>
        <main className="site-container site-page site-prose-page">
            <h1 className="site-article-title mb-10">{title}</h1>
            <RenderMarkdown markdown={markdown} />
        </main>
    </div>
)
