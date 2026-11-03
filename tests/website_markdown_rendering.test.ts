import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire, registerHooks } from 'node:module'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { readMarkdownPage } from '../packages/website/lib/markdown.ts'

const websiteUrl = new URL('../packages/website/', import.meta.url)
const require = createRequire(new URL('package.json', websiteUrl))
const { transpileModule, JsxEmit, ModuleKind } = require('typescript')
const { createElement } = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { JSDOM } = require('jsdom')

// Load the real TSX components and their website imports for React rendering without a server.
const hooks = registerHooks({
    resolve(specifier, context, nextResolve) {
        if (specifier === 'next/link') return nextResolve(require.resolve('next/link.js'), context)
        return nextResolve(specifier.startsWith('@/lib/') ? new URL(specifier.slice(2) + '.ts', websiteUrl).href : specifier, context)
    },
    load(url, context, nextLoad) {
        if (!url.startsWith(websiteUrl.href) || !url.endsWith('.tsx')) return nextLoad(url, context)
        const { outputText } = transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { jsx: JsxEmit.ReactJSX, module: ModuleKind.ESNext } })
        return { format: 'module', source: outputText, shortCircuit: true }
    },
})
const { RenderMarkdown } = await import('../packages/website/components/site/RenderMarkdown.tsx')
const { DocsMarkdown } = await import('../packages/website/components/docs/DocsMarkdown.tsx')
const { chatMarkdownRehypePlugins } = await import('../packages/website/lib/markdownRehypePlugins.ts')
hooks.deregister()
const { Streamdown } = await import(new URL('node_modules/streamdown/dist/index.js', websiteUrl).href)

test('Markdown renderers preserve starting numbers when lists resume after other blocks', () => {
    const markdown = '0. Zero\n\n---\n\n6. First\n7. Second\n\nBetween lists.\n\n9. Last'
    for (const component of [createElement(RenderMarkdown, { markdown }), createElement(DocsMarkdown, { text: markdown, typeAnchorOfName: {}, isMuted: false })]) {
        const dom = new JSDOM(renderToStaticMarkup(component))
        try {
            const lists = [...dom.window.document.querySelectorAll('ol')]
            assert.deepEqual(lists.map(list => list.start), [0, 6, 9])
            assert.deepEqual(lists.map(list => list.children.length), [1, 2, 1])
        } finally {
            dom.window.close()
        }
    }
})

test('blog footnotes keep their targets, return links, and accessibility attributes in the current tab', () => {
    const markdown = 'An explanation[^detail] and an [external source](https://example.com/source).\n\n[^detail]: More detail.'
    const dom = new JSDOM(renderToStaticMarkup(createElement(RenderMarkdown, { markdown })))
    try {
        const document = dom.window.document
        const reference = document.querySelector('a[data-footnote-ref]')
        const backReference = document.querySelector('a[data-footnote-backref]')
        assert.ok(reference)
        assert.ok(backReference)
        assert.equal(reference.getAttribute('aria-describedby'), 'footnote-label')
        assert.ok(backReference.getAttribute('aria-label'))
        for (const link of [reference, backReference]) {
            assert.ok(document.getElementById(link.getAttribute('href').slice(1)))
            assert.equal(link.hasAttribute('target'), false)
            assert.equal(link.hasAttribute('rel'), false)
            assert.ok(link.classList.contains('no-underline'))
        }
        assert.equal(backReference.getAttribute('href'), `#${reference.id}`)
        const externalLink = document.querySelector('a[href="https://example.com/source"]')
        assert.equal(externalLink?.getAttribute('target'), '_blank')
        assert.equal(externalLink?.getAttribute('rel'), 'noreferrer')
    } finally {
        dom.window.close()
    }
})

test('docs type references retain their fragment destinations and muted styles', () => {
    for (const isMuted of [false, true]) {
        const dom = new JSDOM(renderToStaticMarkup(createElement(DocsMarkdown, { text: 'Returns ObjectInfo.', typeAnchorOfName: { ObjectInfo: 'object-info' }, isMuted })))
        try {
            const link = dom.window.document.querySelector('a')
            assert.equal(link?.getAttribute('href'), '#object-info')
            assert.equal(link?.textContent, 'ObjectInfo')
            assert.equal(link?.hasAttribute('target'), false)
            assert.ok(link?.classList.contains(isMuted ? 'text-gray-500' : 'text-foreground'))
        } finally {
            dom.window.close()
        }
    }
})

// A chat message from a guest or a /create prompt reaches other people's browsers on kube.new, so its HTML must stay text.
test('chat shows HTML from a message as text and never as page elements', () => {
    const browserEditorSource = readFileSync(new URL('app/editor/[game_id]/BrowserEditor.tsx', websiteUrl), 'utf8')
    assert.equal(browserEditorSource.match(/<Streamdown\b/g)?.length, 1)
    assert.match(browserEditorSource, /<Streamdown\b[^\n]*rehypePlugins=\{chatMarkdownRehypePlugins\}/)
    const attacks = [
        '<iframe srcdoc="<script>parent.document.title = 1</script>"></iframe>',
        '<script>document.title = 1</script>',
        '<style>html { display: none }</style>',
        '<meta http-equiv="refresh" content="0;url=https://example.com/">',
        '<link rel="stylesheet" href="https://example.com/x.css">',
        '<base href="https://example.com/">',
        '<form action="https://example.com/"><button>Go</button></form>',
        '<object data="https://example.com/x.html"></object>',
        '<embed src="https://example.com/x.svg">',
        '<svg onload="document.title = 1"></svg>',
        '<img src="x" onerror="document.title = 1">',
        '<details open ontoggle="document.title = 1">x</details>',
        '<a href="javascript:document.title = 1">x</a>',
    ]
    const render = (text: string) => new JSDOM(renderToStaticMarkup(createElement(Streamdown, { mode: 'static', controls: false, rehypePlugins: chatMarkdownRehypePlugins }, text)))
    for (const attack of attacks) {
        for (const message of [attack, `Inline ${attack} text`, `| Cell |\n|---|\n| ${attack} |`, `See[^note].\n\n[^note]: ${attack}`]) {
            const dom = render(message)
            try {
                const document = dom.window.document
                assert.equal(document.querySelector('iframe, script, style, meta, link, base, form, button, object, embed, svg, img, details, a[href^="javascript" i]'), null, message)
                assert.ok(document.body.textContent.includes(attack), message)
            } finally {
                dom.window.close()
            }
        }
    }

    // Everything Markdown itself can write still works, and text that only looks like a tag stays in the sentence.
    const dom = render('**Bold** [link](https://example.com/) ![image](https://example.com/i.png) [script](javascript:document.title=1)\n\n| Cell |\n|---|\n| 1 |\n\nIt returns Promise<void>.')
    try {
        const document = dom.window.document
        assert.equal(document.querySelector('[data-streamdown="strong"]')?.textContent, 'Bold')
        assert.equal(document.querySelector('a')?.getAttribute('href'), 'https://example.com/')
        assert.equal(document.querySelector('a[href^="javascript" i]'), null)
        assert.equal(document.querySelector('img')?.getAttribute('src'), 'https://example.com/i.png')
        assert.equal(document.querySelector('td')?.textContent, '1')
        assert.ok(document.body.textContent.includes('It returns Promise<void>.'))
    } finally {
        dom.window.close()
    }
})

for (const pageName of ['terms', 'privacy']) {
    test(`${pageName} renders consecutive numbered sections with separate body paragraphs`, async () => {
        const { markdown } = await readMarkdownPage(fileURLToPath(new URL(`app/${pageName}/${pageName}.md`, websiteUrl)))
        const dom = new JSDOM(renderToStaticMarkup(createElement(RenderMarkdown, { markdown })))
        try {
            const document = dom.window.document
            const headings = [...document.querySelectorAll('h2')]
            const numberedHeadings = headings.filter(heading => /^\d+\./.test(heading.textContent))
            assert.ok(numberedHeadings.length > 0)
            assert.deepEqual(numberedHeadings.map(heading => Number.parseInt(heading.textContent)), numberedHeadings.map((_, index) => index + 1))
            assert.equal(document.querySelectorAll('ol').length, 0, 'Policy section numbers should be headings, not independent lists')
            assert.ok(headings.some(heading => heading.textContent === 'Contact Us'))
            for (const heading of headings) {
                assert.equal(heading.nextElementSibling?.tagName, 'P', `${heading.textContent} needs a separate body paragraph`)
                assert.ok(heading.nextElementSibling.textContent.trim().length > 0)
            }
        } finally {
            dom.window.close()
        }
    })
}
