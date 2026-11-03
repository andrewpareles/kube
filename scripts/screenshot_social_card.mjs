// Takes the picture shown when a link to a page without its own picture is pasted somewhere: the live
// kube.new homepage as a logged-out visitor sees it, not this checkout.
//
//     node scripts/screenshot_social_card.mjs

import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { SITE_DEFAULT_IMAGE_PATH, SITE_ORIGIN } from '../packages/website/lib/siteMetadata.ts'

const websiteDirectory = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'packages', 'website')
const require = createRequire(path.join(websiteDirectory, 'package.json'))
const { chromium } = require('playwright')
const sharp = require('sharp')

const SOCIAL_CARD_WIDTH = 1200
const SOCIAL_CARD_HEIGHT = 630
// TODO: AI-PICKED-VALUE: drawn at 2x and scaled down so the text stays sharp; JPEG quality 88 keeps the card near 90 KB.
const SCREENSHOT_SCALE = 2
const SOCIAL_CARD_JPEG_QUALITY = 88

const browser = await chromium.launch({ channel: 'chrome', headless: true })
try {
    const page = await browser.newPage({ viewport: { width: SOCIAL_CARD_WIDTH, height: SOCIAL_CARD_HEIGHT }, deviceScaleFactor: SCREENSHOT_SCALE, reducedMotion: 'reduce' })
    await page.goto(SITE_ORIGIN, { waitUntil: 'networkidle' })
    const screenshotPng = await page.screenshot()
    await sharp(screenshotPng)
        .resize(SOCIAL_CARD_WIDTH, SOCIAL_CARD_HEIGHT, { kernel: 'lanczos3' })
        .jpeg({ quality: SOCIAL_CARD_JPEG_QUALITY })
        .toFile(path.join(websiteDirectory, 'public', SITE_DEFAULT_IMAGE_PATH))
} finally {
    await browser.close()
}
