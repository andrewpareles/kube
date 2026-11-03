import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { readFile } from 'node:fs/promises'
import { repoPath } from './testPaths.ts'

// The desktop client draws script GUIs, chat and the splash screen in this page. The client reads keys and the
// mouse from the OS itself, so the page tells it where its UI is and whether chat has focus. This runs the
// bundle the client ships in real Chrome with real input.
// Build it first with `npm --prefix packages/play-webview run bundle`.
test('the play webview draws script GUIs with Tailwind and tells the host where its UI is', { skip: process.env.PLAY_WEBVIEW_BROWSER_TESTS !== '1', timeout: 60000 }, async () => {
    const require = createRequire(repoPath('packages/website/package.json'))
    const { chromium } = require('playwright')
    const fileOfPath: { [path: string]: { body: Buffer, contentType: string } } = {
        '/': { body: await readFile(repoPath('packages/play-webview/index.html')), contentType: 'text/html' },
        '/play-webview.bundle.js': { body: await readFile(repoPath('packages/play-webview/dist/play-webview.bundle.js')), contentType: 'text/javascript' },
        '/fonts/InterVariable.ttf': { body: await readFile(repoPath('assets/fonts/InterVariable.ttf')), contentType: 'font/ttf' },
        '/fonts/InterVariable-Italic.woff2': { body: await readFile(repoPath('assets/fonts/InterVariable-Italic.woff2')), contentType: 'font/woff2' },
        '/fonts/GeistMono-Regular.ttf': { body: await readFile(repoPath('assets/fonts/GeistMono-Regular.ttf')), contentType: 'font/ttf' },
    }
    const browserErrors: string[] = []
    const browser = await chromium.launch({ channel: 'chrome', headless: true })
    try {
        const page = await browser.newPage({ viewport: { width: 1000, height: 700 } })
        page.on('pageerror', (error: Error) => browserErrors.push(error.message))
        page.on('console', (message: { type: () => string, text: () => string }) => { if (message.type() === 'error') browserErrors.push(message.text()) })
        await page.route('http://kube-play.localhost/**', (route: { request: () => { url: () => string }, fulfill: (response: object) => Promise<void> }) => {
            const file = fileOfPath[new URL(route.request().url()).pathname]
            return file === undefined ? route.fulfill({ status: 404 }) : route.fulfill({ body: file.body, contentType: file.contentType })
        })
        // wry gives the real page an ipc object; this one records what the page sends the host. The page sends where
        // its UI is whenever it moves, so only the latest of those is kept.
        await page.addInitScript(() => {
            const sentToHost: unknown[] = []
            const pageWindow = window as unknown as { latestUiRects: unknown }
            const postMessage = (message: string) => {
                const parsedMessage = JSON.parse(message)
                if (typeof parsedMessage === 'object' && parsedMessage !== null && 'setUiRects' in parsedMessage) {
                    pageWindow.latestUiRects = parsedMessage.setUiRects
                    return
                }
                sentToHost.push(parsedMessage)
            }
            Object.assign(window, { sentToHost, latestUiRects: null, ipc: { postMessage } })
        })
        const takeSentToHost = () => page.evaluate(() => (window as unknown as { sentToHost: unknown[] }).sentToHost.splice(0))
        type Rect = { x: number, y: number, width: number, height: number }
        type UiRects = { uiOnlyRects: Rect[], scrollableRects: Rect[] }
        const getLatestUiRects = () => page.evaluate(() => (window as unknown as { latestUiRects: UiRects | null }).latestUiRects)
        const isSameRect = (rect: Rect, otherRect: Rect) => Math.abs(rect.x - otherRect.x) < 0.5 && Math.abs(rect.y - otherRect.y) < 0.5 && Math.abs(rect.width - otherRect.width) < 0.5 && Math.abs(rect.height - otherRect.height) < 0.5
        const waitForUiRects = async (isExpected: (uiRects: UiRects) => boolean) => {
            const deadline = Date.now() + 5000
            while (Date.now() < deadline) {
                const uiRects = await getLatestUiRects()
                if (uiRects !== null && isExpected(uiRects)) {
                    return
                }
                await page.waitForTimeout(20)
            }
            assert.fail(`the page never sent the expected UI rects: ${JSON.stringify(await getLatestUiRects())}`)
        }
        const receiveFromHost = (message: unknown) => page.evaluate((message: unknown) => (window as unknown as { receiveFromHost: (message: unknown) => void }).receiveFromHost(message), message)

        await page.goto('http://kube-play.localhost/')
        await page.waitForFunction(() => (window as unknown as { sentToHost: unknown[] }).sentToHost.includes('ready'))
        await takeSentToHost()

        const text = (nodeId: string, text: string) => ({ text: { nodeId, text } })
        const noHandlerIds = { onClickHandlerId: null, onRightClickHandlerId: null, onMouseDownHandlerId: null, onMouseUpHandlerId: null, onHoverHandlerId: null }
        const element = (elementKind: string, nodeId: string, className: string, children: unknown[], handlerIdOfFieldName: { [fieldName: string]: string }) =>
            ({ element: { elementKind, nodeId, className, ...noHandlerIds, ...handlerIdOfFieldName, children } })
        await receiveFromHost('showPlayScreen')
        const hud = (fillWidthClassName: string) => ({ setScriptGuiRoots: [{
            scriptObjectId: 7, guiKey: 'hud', root: { children: [
                element('div', 'panel', 'absolute top-4 right-4 flex w-[400px] flex-col gap-3 bg-slate-900 p-4 text-white', [
                    element('div', 'track', 'h-4 w-full bg-black', [element('div', 'fill', `h-full ${fillWidthClassName} bg-emerald-400`, [], {})], {}),
                    element('button', 'buy', 'h-10 bg-sky-500 hover:bg-sky-400', [text('buy.text', 'Buy')], { onClickHandlerId: 'buy.onClick' }),
                    element('div', 'card', 'h-10 bg-white/10', [text('card.text', 'Card')], { onClickHandlerId: 'card.onClick', onHoverHandlerId: 'card.onHover' }),
                    element('div', 'slot', 'flex h-10 items-center justify-between bg-white/20', [
                        text('slot.text', 'Slot'),
                        element('div', 'gem', 'size-6 bg-amber-400', [], { onMouseDownHandlerId: 'gem.onMouseDown' }),
                    ], { onRightClickHandlerId: 'slot.onRightClick', onMouseDownHandlerId: 'slot.onMouseDown', onMouseUpHandlerId: 'slot.onMouseUp' }),
                    element('div', 'bag', 'h-10 bg-white/30', [text('bag.text', 'Bag')], { onMouseUpHandlerId: 'bag.onMouseUp' }),
                    element('div', 'list', 'h-16 overflow-y-auto', [
                        element('div', 'rows', 'h-64', [], {}),
                        element('button', 'more', 'h-10', [text('more.text', 'More')], { onClickHandlerId: 'more.onClick' }),
                    ], {}),
                ], {}),
            ] },
        }] })
        await receiveFromHost(hud('w-[37%]'))
        await receiveFromHost({ setGameChat: { entries: [{ PlayerJoined: { username: 'Alice' } }], error: null, isDisconnected: false, maxMessageChars: 500 } })
        await page.getByText('Alice joined').waitFor()
        assert.deepEqual(await takeSentToHost(), [{ setVolume: 1 }, { setGraphicsQuality: 'low' }, { setIsMenuOpen: false }])

        // Any Tailwind class works, including arbitrary values scripts build while they run.
        const fillWidth = await page.locator('.w-\\[37\\%\\]').evaluate((fill: HTMLElement) => fill.getBoundingClientRect().width / fill.parentElement!.getBoundingClientRect().width)
        assert.ok(Math.abs(fillWidth - 0.37) < 0.01, `fill width ${fillWidth}`)
        const buy = page.getByRole('button', { name: 'Buy' })
        assert.notEqual(await buy.evaluate((button: HTMLElement) => getComputedStyle(button).backgroundColor), 'rgba(0, 0, 0, 0)')

        // A meter's new width only adds the CSS for its class. Every rule already up stays as it is, so the browser never
        // redoes what it set up for one, like loading a font, which made text flicker when the whole stylesheet went up again.
        const getStylesheetRules = () => page.evaluateHandle(() => {
            const getRules = (list: CSSStyleSheet | CSSGroupingRule): CSSRule[] => Array.from(list.cssRules).flatMap((rule) => [rule, ...(rule instanceof CSSGroupingRule ? getRules(rule) : [])])
            return getRules(document.adoptedStyleSheets[0])
        })
        const rulesBefore = await getStylesheetRules()
        await receiveFromHost(hud('w-[38%]'))
        const newFillWidth = await page.locator('.w-\\[38\\%\\]').evaluate((fill: HTMLElement) => fill.getBoundingClientRect().width / fill.parentElement!.getBoundingClientRect().width)
        assert.ok(Math.abs(newFillWidth - 0.38) < 0.01, `fill width ${newFillWidth}`)
        const rulesAfter = await getStylesheetRules()
        assert.deepEqual(await page.evaluate(([rulesBefore, rulesAfter]: CSSRule[][]) => rulesBefore.filter((rule) => !rulesAfter.includes(rule)).map((rule) => rule.cssText), [rulesBefore, rulesAfter]), [])

        // A creator's class name cannot make the page load anything, or every player's address would reach the creator's server.
        const requestedUrls: string[] = []
        page.on('request', (request: { url: () => string }) => requestedUrls.push(request.url()))
        const refusedLoad = page.waitForEvent('console', (message: { text: () => string }) => message.text().includes('tracker.localhost'))
        await receiveFromHost(hud('w-[39%] bg-[url(http://tracker.localhost/player.png)]'))
        await refusedLoad
        assert.deepEqual(requestedUrls, [])
        assert.equal(browserErrors.splice(0).length, 1)

        // The host is told where the buttons and chat are, which keep presses to themselves, and where the panel
        // that can scroll is. A button scrolled out of its panel cannot be pressed, so it only counts once it shows.
        // A div keeps presses too once it has a click or mouse button handler, but the track and fill do not.
        const buyRect = (await buy.boundingBox())!
        const cardRect = (await page.getByText('Card').boundingBox())!
        const chatRect = (await page.locator('[data-ui-only]').filter({ has: page.getByPlaceholder('Press / to chat') }).boundingBox())!
        const listRect = (await page.locator('.h-16').boundingBox())!
        await waitForUiRects(({ uiOnlyRects, scrollableRects }) =>
            uiOnlyRects.length === 9
            && uiOnlyRects.some(rect => isSameRect(rect, buyRect))
            && uiOnlyRects.some(rect => isSameRect(rect, cardRect))
            && uiOnlyRects.some(rect => isSameRect(rect, chatRect))
            && scrollableRects.length === 1 && isSameRect(scrollableRects[0], listRect))
        await page.locator('.h-16').evaluate((list: HTMLElement) => { list.scrollTop = list.scrollHeight })
        const moreRect = (await page.getByRole('button', { name: 'More' }).boundingBox())!
        await waitForUiRects(({ uiOnlyRects }) => uiOnlyRects.length === 10 && uiOnlyRects.some(rect => isSameRect(rect, { ...moreRect, height: listRect.y + listRect.height - moreRect.y })))

        // The host reads presses and keys from the OS, so the page only runs the GUI's own handlers.
        const guiHandler = (handlerId: string, button: string | null) => ({ guiHandler: { scriptObjectId: 7, guiKey: 'hud', handlerId, button } })
        await buy.click()
        assert.deepEqual(await takeSentToHost(), [guiHandler('buy.onClick', null)])
        await page.getByText('Card').click()
        assert.deepEqual(await takeSentToHost(), [guiHandler('card.onHover', null), guiHandler('card.onClick', null)])
        await buy.click({ button: 'right' })
        assert.deepEqual(await takeSentToHost(), [])

        // Mouse button handlers get the button, and each press or release goes to the innermost element with a handler
        // for it. A right click needs the button to go down and come back up in the same element.
        const pressAndRelease = async (button: 'left' | 'right' | 'middle', pressPosition: { x: number, y: number }, releasePosition: { x: number, y: number }) => {
            await page.mouse.move(pressPosition.x, pressPosition.y)
            await page.mouse.down({ button })
            await page.mouse.move(releasePosition.x, releasePosition.y)
            await page.mouse.up({ button })
            return takeSentToHost()
        }
        const getCenter = async (text: string) => {
            const box = (await page.getByText(text, { exact: true }).boundingBox())!
            return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
        }
        const slotPosition = await getCenter('Slot')
        const gemBox = (await page.locator('.bg-amber-400').boundingBox())!
        const gemPosition = { x: gemBox.x + gemBox.width / 2, y: gemBox.y + gemBox.height / 2 }
        const bagPosition = await getCenter('Bag')
        const worldPosition = { x: 500, y: 650 }
        assert.deepEqual(await pressAndRelease('right', slotPosition, slotPosition), [guiHandler('slot.onMouseDown', 'right'), guiHandler('slot.onMouseUp', 'right'), guiHandler('slot.onRightClick', null)])
        assert.deepEqual(await pressAndRelease('right', gemPosition, gemPosition), [guiHandler('gem.onMouseDown', 'right'), guiHandler('slot.onMouseUp', 'right'), guiHandler('slot.onRightClick', null)])
        assert.deepEqual(await pressAndRelease('left', gemPosition, bagPosition), [guiHandler('gem.onMouseDown', 'left'), guiHandler('bag.onMouseUp', 'left')])
        assert.deepEqual(await pressAndRelease('right', slotPosition, bagPosition), [guiHandler('slot.onMouseDown', 'right'), guiHandler('bag.onMouseUp', 'right')])
        assert.deepEqual(await pressAndRelease('right', worldPosition, slotPosition), [guiHandler('slot.onMouseUp', 'right')])
        assert.deepEqual(await pressAndRelease('middle', slotPosition, slotPosition), [])
        await page.keyboard.press('KeyW')
        assert.deepEqual(await takeSentToHost(), [])

        // / opens the chat without typing a slash, and the host is told typing is not game input until Enter sends and leaves.
        await page.keyboard.press('Slash')
        const chatInput = page.getByPlaceholder('Press / to chat')
        assert.equal(await chatInput.evaluate((input: HTMLInputElement) => document.activeElement === input), true)
        await page.keyboard.type('  hello  ')
        await page.keyboard.press('Enter')
        assert.deepEqual(await takeSentToHost(), [{ setIsChatFocused: true }, { sendGameChatMessage: { message: 'hello' } }, { setIsChatFocused: false }])
        assert.equal(await chatInput.inputValue(), '')
        assert.equal(await chatInput.evaluate((input: HTMLInputElement) => document.activeElement === input), false)

        // Pasted control characters are refused before sending, and the draft stays to fix.
        await chatInput.click()
        await chatInput.fill('hello\tworld')
        await page.keyboard.press('Enter')
        assert.deepEqual(await takeSentToHost(), [{ setIsChatFocused: true }])
        assert.equal(await chatInput.inputValue(), 'hello\tworld')

        // The chat button hides the chat and keeps the draft, and / brings it back to type in.
        await page.getByRole('button', { name: 'Chat' }).click()
        assert.equal(await chatInput.isVisible(), false)
        await page.keyboard.press('Slash')
        assert.equal(await chatInput.evaluate((input: HTMLInputElement) => document.activeElement === input), true)
        assert.equal(await chatInput.inputValue(), 'hello\tworld')
        assert.deepEqual(await takeSentToHost(), [{ setIsChatFocused: false }, { setIsChatFocused: true }])

        // The menu button opens the menu over everything, and pressing outside its panel closes it.
        await page.getByRole('button', { name: 'Menu' }).click()
        const menuTitle = page.getByText('Settings', { exact: true })
        await menuTitle.waitFor()
        await page.mouse.click(5, 690)
        await menuTitle.waitFor({ state: 'detached' })
        assert.deepEqual(await takeSentToHost(), [{ setIsChatFocused: false }, { setIsMenuOpen: true }, { setIsMenuOpen: false }])

        // Escape opens and closes the menu too.
        await page.keyboard.press('Escape')
        await menuTitle.waitFor()
        await page.keyboard.press('Escape')
        await menuTitle.waitFor({ state: 'detached' })
        // The host is told each time, since it lets go of a locked mouse while the menu is open.
        assert.deepEqual(await takeSentToHost(), [{ setIsMenuOpen: true }, { setIsMenuOpen: false }])

        // The menu keeps its volume and graphics settings while it is closed, and Escape still closes it after the slider is used.
        await page.getByRole('button', { name: 'Menu' }).click()
        await page.getByRole('slider', { name: /Volume/ }).fill('0.4')
        await page.getByText('40%').waitFor()
        await page.getByRole('button', { name: 'Ultra' }).click()
        assert.deepEqual(await takeSentToHost(), [{ setIsMenuOpen: true }, { setVolume: 0.4 }, { setGraphicsQuality: 'high' }])
        await page.getByRole('slider', { name: /Volume/ }).focus()
        await page.keyboard.press('Escape')
        await menuTitle.waitFor({ state: 'detached' })
        await page.keyboard.press('Escape')
        await page.getByText('40%').waitFor()
        assert.equal(await page.getByRole('button', { name: 'Ultra' }).getAttribute('aria-pressed'), 'true')
        await page.keyboard.press('Escape')
        await menuTitle.waitFor({ state: 'detached' })
        assert.deepEqual(await takeSentToHost(), [{ setIsMenuOpen: false }, { setIsMenuOpen: true }, { setIsMenuOpen: false }])

        // Leave asks first, and only then asks the host to close the game.
        await page.getByRole('button', { name: 'Menu' }).click()
        assert.deepEqual(await takeSentToHost(), [{ setIsMenuOpen: true }])
        await page.getByRole('button', { name: 'Leave' }).click()
        await page.getByText('Leave this game?').waitFor()
        assert.deepEqual(await takeSentToHost(), [])
        await page.getByRole('button', { name: 'Leave' }).last().click()
        assert.deepEqual(await takeSentToHost(), ['leaveGame'])
        await page.keyboard.press('Escape')

        // Holding Tab shows every player in the game, with the player's own name first, until Tab comes back up.
        await receiveFromHost({ setPlayersList: [{ username: 'Alice', isLocalPlayer: false, pingMs: 42 }, { username: 'Bob', isLocalPlayer: true, pingMs: null }] })
        await page.keyboard.down('Tab')
        const bobInPlayersList = page.getByText('Bob', { exact: true })
        await bobInPlayersList.waitFor()
        assert.equal(await bobInPlayersList.evaluate((username: HTMLElement) => username.parentElement?.previousElementSibling), null)
        await page.getByText('42 ms', { exact: true }).waitFor()
        assert.equal(await bobInPlayersList.evaluate((username: HTMLElement) => username.parentElement?.textContent), 'Bob')
        await page.keyboard.up('Tab')
        assert.equal(await bobInPlayersList.count(), 0)

        // A splash screen replaces the game's GUI and chat, and with them all the UI the host checks presses against.
        await receiveFromHost({ showSplashScreen: { titleText: 'Disconnected', subText: 'The server stopped.', thumbnailUrl: null, isLoading: false } })
        await page.getByText('The server stopped.').waitFor()
        assert.equal(await buy.count(), 0)
        await waitForUiRects(({ uiOnlyRects, scrollableRects }) => uiOnlyRects.length === 0 && scrollableRects.length === 0)
        assert.deepEqual(browserErrors, [])
    } finally {
        await browser.close()
    }
})
