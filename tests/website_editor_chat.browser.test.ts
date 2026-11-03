import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { mkdtemp, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { repoPath } from './testPaths.ts'

// Real browser layout, wheel input, and ResizeObserver exercise the production chat container without a server.
test('chat follows streaming content, pauses for reading, and resumes at the bottom', { skip: process.env.EDITOR_CHAT_BROWSER_TESTS !== '1', timeout: 60000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const { build } = require('esbuild')
    const postcss = require('postcss')
    const tailwind = require('@tailwindcss/postcss')
    const website = repoPath('packages/website')
    const directory = await mkdtemp(path.join(tmpdir(), 'editor-chat-browser-'))
    const browserErrors: string[] = []
    let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined
    try {
        await build({
            stdin: { contents: `
                import React, { useLayoutEffect, useState } from 'react'
                import { createRoot } from 'react-dom/client'
                import { EditorChatScroll } from '@/app/editor/[game_id]/EditorChatScroll'
                const App = () => {
                    const [response, setResponse] = useState('Current response. ')
                    useLayoutEffect(() => { window.chatTest = { append: text => setResponse(previous => previous + text) } }, [])
                    return <main style={{ display: 'flex', flexDirection: 'column', height: 400, width: 360, marginTop: 120 }}>
                        <EditorChatScroll>
                            {Array.from({ length: 20 }, (_, index) => <p key={index}>Earlier message {index}: a paragraph that wraps when the chat panel is resized.</p>)}
                            <p>{response}</p>
                            <img alt="Tool preview" width="200" height="20" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='20'/%3E" />
                        </EditorChatScroll>
                        <textarea aria-label="Message" style={{ height: 80, flexShrink: 0 }} />
                    </main>
                }
                createRoot(document.getElementById('root')).render(<React.StrictMode><App /></React.StrictMode>)
            `, resolveDir: website, loader: 'tsx' },
            outfile: path.join(directory, 'app.js'), bundle: true, format: 'esm', jsx: 'automatic',
            tsconfig: path.join(website, 'tsconfig.json'), define: { 'process.env.NODE_ENV': '"development"' },
        })
        const css = await postcss([tailwind({ base: website })]).process('@import "tailwindcss"; @import "tw-animate-css";', { from: path.join(website, 'editor-chat-test.css') })
        await writeFile(path.join(directory, 'styles.css'), css.css)
        await writeFile(path.join(directory, 'index.html'), '<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="styles.css"></head><body style="min-height:2000px"><div id="root"></div><script type="module" src="app.js"></script></body></html>')
        browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] })
        const page = await browser.newPage({ viewport: { width: 1000, height: 800 }, reducedMotion: 'reduce' })
        page.on('pageerror', (error: Error) => browserErrors.push(error.message))
        page.on('console', (message: { type: () => string, text: () => string }) => { if (message.type() === 'error') browserErrors.push(message.text()) })
        await page.goto(pathToFileURL(path.join(directory, 'index.html')).href)
        const chat = page.getByRole('region', { name: 'Chat messages' })
        const latest = page.getByRole('button', { name: 'Scroll to latest', exact: true })
        // Chrome can finish a keyboard scroll after its first scrollend, so also wait until the position stops changing.
        const scrollWith = async (action: () => Promise<unknown>) => {
            await Promise.all([
                chat.evaluate((element: HTMLElement) => new Promise(resolve => element.addEventListener('scrollend', () => resolve(null), { once: true }))),
                action(),
            ])
            await chat.evaluate((element: HTMLElement) => new Promise(resolve => {
                const settle = (previous: number) => setTimeout(() => element.scrollTop === previous ? resolve(null) : settle(element.scrollTop), 100)
                settle(element.scrollTop)
            }))
        }
        const waitForBottom = () => page.waitForFunction(() => {
            const element = document.querySelector('[aria-label="Chat messages"]')
            return element && element.scrollHeight > element.clientHeight && element.scrollHeight - element.clientHeight - element.scrollTop <= 2
        })
        const append = async () => {
            const height = await chat.evaluate((element: HTMLElement) => element.scrollHeight)
            await page.evaluate('window.chatTest.append("More streamed output. ".repeat(40))')
            await page.waitForFunction((height: number) => document.querySelector('[aria-label="Chat messages"]').scrollHeight > height, height)
            // Let native resize and scroll events settle before comparing positions.
            await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(null)))))
        }
        await waitForBottom()
        assert.equal(await latest.count(), 0)
        await append()
        await waitForBottom()

        await chat.hover()
        await scrollWith(() => page.mouse.wheel(0, -240))
        await latest.waitFor()
        const readingPosition = await chat.evaluate((element: HTMLElement) => element.scrollTop)
        await append()
        await append()
        assert.equal(await chat.evaluate((element: HTMLElement) => element.scrollTop), readingPosition, 'Streaming must not pull the reader away from older messages')

        await latest.click()
        await waitForBottom()
        await append()
        await waitForBottom()
        assert.equal(await latest.count(), 0)

        await chat.focus()
        await scrollWith(() => page.keyboard.press('PageUp'))
        await latest.waitFor()
        const keyboardPosition = await chat.evaluate((element: HTMLElement) => element.scrollTop)
        await append()
        assert.equal(await chat.evaluate((element: HTMLElement) => element.scrollTop), keyboardPosition)
        await scrollWith(() => page.keyboard.press('End'))
        await waitForBottom()
        await latest.waitFor({ state: 'detached' })
        await append()
        await waitForBottom()

        // Layout changes after message delivery must also follow, without a messages-array update.
        await page.getByAltText('Tool preview').evaluate((image: HTMLImageElement) => { image.style.height = '300px' })
        await waitForBottom()
        await page.locator('main').evaluate((element: HTMLElement) => { element.style.width = '260px' })
        await waitForBottom()
        assert.equal(await page.evaluate(() => window.scrollY), 0, 'Only the chat container should scroll')
        assert.deepEqual(browserErrors, [])
    } finally {
        await browser?.close()
        await rm(directory, { recursive: true, force: true })
    }
})
