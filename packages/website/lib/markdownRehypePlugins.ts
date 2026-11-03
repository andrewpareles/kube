import { defaultRehypePlugins, type StreamdownProps } from 'streamdown'

type HastNode = {
    type: string
    tagName?: string
    value?: string
    children?: HastNode[]
}

// Markdown-to-html conversion appends a "\n" to fenced code text, and people often leave a blank
// line just inside the fence, which Streamdown renders as phantom empty lines above and below the code.
const rehypeTrimCodeBlockBlankLines = () => (tree: HastNode) => {
    if (tree.tagName === 'pre') {
        for (const codeNode of tree.children ?? []) {
            if (codeNode.tagName !== 'code') continue
            const firstChild = codeNode.children?.[0]
            const lastChild = codeNode.children?.[codeNode.children.length - 1]
            if (firstChild?.type === 'text' && firstChild.value !== undefined) firstChild.value = firstChild.value.replace(/^(?:[ \t]*\n)+/, '')
            if (lastChild?.type === 'text' && lastChild.value !== undefined) lastChild.value = lastChild.value.replace(/(?:\n[ \t]*)+$/, '')
        }
        return
    }
    for (const child of tree.children ?? []) rehypeTrimCodeBlockBlankLines()(child)
}

// Streamdown checks every URL in the markdown it renders, but it never says which origin a relative
// URL belongs to, and without one the check blocks every image whose src is not a full https:// URL.
// Naming the origin is what lets an image written as /blog/thing.svg through. It is only used to
// resolve the path for that check - the src that reaches the page is still the path as written, so
// this does not tie images to a deployment.
const SITE_ORIGIN = 'https://kube.new'

// Streamdown ships the check as [plugin, options], and we keep its plugin and restate its options
// with the origin added. If a version ever ships it some other way we want to hear about it here
// rather than quietly render blog posts with their images stripped out.
const hardenEntry = defaultRehypePlugins.harden
const hardenPlugin = Array.isArray(hardenEntry) ? hardenEntry[0] : hardenEntry
if (typeof hardenPlugin !== 'function') throw new Error('Streamdown no longer ships its URL check as a plugin, so blog images cannot be allowed through.')

// The blog and the docs render Markdown the same way, so they share one set of rehype plugins.
export const markdownRehypePlugins: NonNullable<StreamdownProps['rehypePlugins']> = [
    ...Object.entries(defaultRehypePlugins).filter(([name]) => name !== 'harden').map(([, plugin]) => plugin),
    [hardenPlugin, { allowedImagePrefixes: ['*'], allowedLinkPrefixes: ['*'], allowDataImages: true, defaultOrigin: SITE_ORIGIN }],
    rehypeTrimCodeBlockBlankLines,
]

// Streamdown drops any HTML the raw plugin has not parsed, which would cut words out of a message.
const rehypeShowRawHtmlAsText = () => (tree: HastNode) => {
    if (tree.type === 'raw') tree.type = 'text'
    for (const child of tree.children ?? []) rehypeShowRawHtmlAsText()(child)
}

// Chat text comes from other people and from AI, so it must not use the raw plugin, which turns HTML in Markdown into real page elements.
export const chatMarkdownRehypePlugins: NonNullable<StreamdownProps['rehypePlugins']> = [
    ...markdownRehypePlugins.filter(plugin => plugin !== defaultRehypePlugins.raw),
    rehypeShowRawHtmlAsText,
]
