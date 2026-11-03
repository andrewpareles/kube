'use client'

import Link from 'next/link'
import { Streamdown } from 'streamdown'
import { markdownRehypePlugins } from '@/lib/markdownRehypePlugins'
import { DESCENDANT_ANCHOR_SCROLL_MARGIN_CLASS_NAME } from '@/lib/siteNavigation'

// Blog posts render the same Markdown as the docs, so they go through the same rehype plugins.
// This is a client component because the plugins are functions, which a server component can't pass on.
export const RenderMarkdown = ({ markdown }: { markdown: string }) => (
    <Streamdown
        mode="static"
        controls={false}
        rehypePlugins={markdownRehypePlugins}
        // A footnote number and the arrow that comes back from it jump to ids inside the post, and every
        // one of those ids has to clear the fixed header the same way an anchor anywhere else on the site does.
        className={`site-prose ${DESCENDANT_ANCHOR_SCROLL_MARGIN_CLASS_NAME}`}
        components={{
            // Footnote numbers and their back arrows point at a spot on this same page, so they jump
            // in place instead of opening a tab. Prose links are underlined, but a footnote marker is
            // punctuation rather than something to read, so it stays quiet and only underlines on hover.
            // rehype-harden stamps target="_blank" onto every anchor it passes, fragment links included,
            // so its target and rel are pulled out of the props here and the spread goes first. Leaving
            // either of those to chance puts a "#footnote" link in a new tab.
            a: ({ node, href, className, children, target: hardenTarget, rel: hardenRel, ...props }) => {
                const isSamePageAnchor = href !== undefined && href.startsWith('#')
                const isFootnoteMarker = 'data-footnote-ref' in props || 'data-footnote-backref' in props
                const emphasisClassName = isFootnoteMarker ? 'no-underline hover:underline' : 'underline underline-offset-2'
                const linkClassName = `wrap-anywhere text-primary ${emphasisClassName} ` + (className ?? '')
                // Next must record footnote jumps so Back can restore this page after navigating away.
                if (isSamePageAnchor) return <Link {...props} href={href} className={linkClassName}>{children}</Link>
                return (
                    <a
                        {...props}
                        href={href}
                        target="_blank"
                        rel="noreferrer"
                        className={linkClassName}
                    >
                        {children}
                    </a>
                )
            },
            img: ({ node, className, alt, ...props }) => (
                // eslint-disable-next-line @next/next/no-img-element
                <img alt={alt}
                    className={'my-8 h-auto max-w-full sm:max-w-xl rounded-lg mx-auto ' + (className ?? '')}
                    {...props}
                />
            ),
            // Streamdown puts the list marker inside the item, which drops the "1." onto its own line
            // whenever the item holds a paragraph. Hanging the marker outside keeps it beside the text.
            // Preserve the starting number when Markdown continues a list after another block.
            ol: ({ children, start }) => <ol start={start} className="list-outside list-decimal pl-5">{children}</ol>,
            ul: ({ children }) => <ul className="list-outside list-disc pl-5">{children}</ul>,
        }}
    >
        {markdown}
    </Streamdown>
)
