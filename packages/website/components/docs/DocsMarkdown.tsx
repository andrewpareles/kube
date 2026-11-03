'use client'

import Link from 'next/link'
import { Streamdown } from 'streamdown'
import { getDocsMarkdownWithTypeLinks } from '@/lib/docsMarkdown'
import { markdownRehypePlugins } from '@/lib/markdownRehypePlugins'
import { ANCHOR_SCROLL_MARGIN_CLASS_NAME, DESCENDANT_ANCHOR_SCROLL_MARGIN_CLASS_NAME } from '@/lib/siteNavigation'

// Human docs and API references share the same Markdown rendering.
type DocsMarkdownProps = {
    text: string
    typeAnchorOfName: { [name: string]: string }
    isMuted: boolean
}

export const DocsMarkdown = ({ text, typeAnchorOfName, isMuted }: DocsMarkdownProps) => (
    <Streamdown
        mode="static"
        controls={false}
        rehypePlugins={markdownRehypePlugins}
        className={(isMuted ? 'space-y-2 text-gray-400' : 'space-y-2 text-foreground') + ` [&_code]:text-xs [&_pre]:text-xs ${DESCENDANT_ANCHOR_SCROLL_MARGIN_CLASS_NAME}`}
        components={{
            a: ({ href, children }) => {
                const className = (isMuted ? 'text-gray-500' : 'text-foreground') + ' underline decoration-dotted underline-offset-2'
                // Type links must keep Next's history state when jumping within the API page.
                if (href !== undefined && href.startsWith('#')) return <Link href={href} className={className}>{children}</Link>
                return <a href={href} className={className}>{children}</a>
            },
            h2: ({ children }) => <h2 className={`mb-3 mt-10 ${ANCHOR_SCROLL_MARGIN_CLASS_NAME} text-xl font-semibold first:mt-0`}>{children}</h2>,
            ol: ({ children, start }) => <ol start={start} className="list-outside list-decimal space-y-1 pl-5">{children}</ol>,
            ul: ({ children }) => <ul className="list-disc space-y-1 pl-5">{children}</ul>,
            li: ({ children }) => <li>{children}</li>,
        }}
    >
        {getDocsMarkdownWithTypeLinks(text, typeAnchorOfName)}
    </Streamdown>
)
