// Draws every fixed image of the mark: the website's favicon and app icons, and the source image
// the desktop app's icons are built from.
//
//     npm run generate-icons
//
// There is one drawing here and everything is that drawing. It is rendered once, large, and every
// output is that one render scaled down and dropped onto whatever background it needs. Nothing
// gets its own weight or its own camera angle, so a favicon is a small copy of the app icon is a
// small copy of the mark in the header, rather than three drawings that merely look alike.
//
// Two things follow from that and are worth knowing before changing them:
//
//   - The one thing that does change with size is the stroke, and it changes by the size the image
//     will be *looked at* rather than the size of the file. A hairline that reads as a line at 200
//     pixels disappears at 16, so the small ones are drawn heavier. Every output below says how big
//     it will be seen and the weight follows from that, which is why the app icon no longer looks
//     fatter than the mark on the site: they are both asking the same question and getting the
//     same answer.
//   - Small images are never asked for directly. Handing a 16 pixel canvas to an SVG renderer and
//     asking it to draw masks, gradients and hairlines gives back a scratchy mess. Every size is
//     drawn large and resampled down instead.
//
// The mark is levelled, the same as the one in the site header: at these sizes a leg that drops a
// pixel across its width just looks badly drawn.

import { writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
    getRestingMarkBounds,
    renderRestingKubeLogoSvg,
    restingKView,
    restingLogoPreset,
} from '../../packages/website/lib/kubeLogo/kubeLogoRender.ts'

const iconGenDirectory = path.dirname(fileURLToPath(import.meta.url))
const repositoryDirectory = path.join(iconGenDirectory, '..', '..')
const websiteDirectory = path.join(repositoryDirectory, 'packages', 'website')
// Where the packager keeps the art it builds installers out of, one folder per platform.
const windowsPackagingAssetsDirectory = path.join(repositoryDirectory, 'packaging', 'client', 'assets', 'windows')

// The drawing comes out of the website's own logo module, and so does the tool that turns it into
// images: sharp is installed there. Asked for by name from here, Node would look in this folder
// and in the repository root and find nothing, so it is asked for against the website instead.
const sharp = createRequire(path.join(websiteDirectory, 'package.json'))('sharp')

// TODO: AI-PICKED-VALUE: Big enough that the largest thing made from it, the desktop app's 1024
// pixel icon source, is a straight copy rather than something scaled up.
const MASTER_RENDER_SIZE = 1024
// TODO: AI-PICKED-VALUE: How much of a square icon the mark itself takes up. The rest is clear
// space, which keeps the K off the rounded corners that platforms crop icons to.
// (this is for the website)
const ICON_MARK_FRACTION = 0.78
// TODO: AI-PICKED-VALUE: A favicon is never cropped or masked by anything, so the K fills it, with
// only enough left over for the edge of the stroke to soften into rather than be cut off by.
const FAVICON_MARK_FRACTION = 0.94
// Desktop icons sit in a dock beside other rounded icons, so this one is drawn as one, using the
// corner radius macOS uses. The mark fills much more of it than it used to, though still less than
// it fills a favicon: a favicon has no tile to sit on, and this one has rounded corners to stay off.
const DESKTOP_ICON_MARK_FRACTION = 0.68
const DESKTOP_ICON_CORNER_FRACTION = 0.2237
// TODO: AI-PICKED-VALUE: The desktop icon is the one mark drawn heavier than its size alone asks
// for. It is the only one sitting on a solid tile among other dock icons, and beside those a K at
// the weight the curve gives reads as thin, so its stroke is drawn this much thicker.
const DESKTOP_ICON_STROKE_WEIGHT_MULTIPLIER = 1.25
// TODO: AI-PICKED-VALUE: Roughly the size a desktop icon is looked at in a dock or a task bar.
// One source image has to serve every size the packager cuts out of it, so it is drawn for the
// size it is seen at most.
const DESKTOP_ICON_DISPLAY_SIZE = 128
// Windows draws an app icon straight onto the task bar and the desktop with nothing behind it and
// nothing cropping it, so the Windows icon is the same mark drawn at the same weight as the macOS
// one but with the tile taken away. With no rounded corners to stay clear of, the only reason left
// to hold the mark back from the edge is so it does not crowd its neighbours in a task bar.
// TODO: AI-PICKED-VALUE: How much of the square the mark fills once there is no tile under it.
const WINDOWS_ICON_MARK_FRACTION = 0.9

// Icons are see-through, so the mark sits on whatever it is put on. The only lavender in them is
// the mark's own faces, which the generator paints. Where see-through cannot work — an icon a
// platform composites onto black, or one that has to read as a solid tile in a dock — it is white
// instead rather than a colour of its own.
const OPAQUE_ICON_BACKGROUND = '#ffffff'

// -------------------------- How heavy the stroke is --------------------------

/**
 * The stroke weight for a mark that will be looked at this big.
 *
 * These two numbers are not taste. They are fitted to the weights the site itself already uses —
 * 1.625 at 34 pixels, 1.51 at 40, 1.4 at 46, and 1 at 196 — so a mark made here at a given size
 * comes out identical to a mark the site draws at that size. Never below the authored weight: the
 * curve keeps thinning above 200 pixels and there is nothing to gain from a fainter mark.
 */
const getStrokeScaleForDisplaySize = (displaySidePx) => Math.max(1, 0.869 + 25.7 / displaySidePx)

// -------------------------- The one drawing --------------------------

/**
 * The mark, cropped out of the square the generator draws it in. That square is sized for a whole
 * cube, so a K sits in the middle of a lot of nothing; this re-aims the drawing at the mark alone
 * and squares it up, so an icon comes out square whichever way round the mark is.
 */
const renderMasterMarkSvg = ({ displaySidePx, strokeWeightMultiplier }) => {
    const strokeScale = getStrokeScaleForDisplaySize(displaySidePx) * (strokeWeightMultiplier ?? 1)
    const markBounds = getRestingMarkBounds({ view: restingKView, strokeScale, isKLevel: true, preset: restingLogoPreset })
    const svgSource = renderRestingKubeLogoSvg({
        view: restingKView,
        renderId: 'kube-mark',
        sidePx: MASTER_RENDER_SIZE,
        strokeScale,
        isKLevel: true,
        preset: restingLogoPreset,
    })

    // The bounds come back as fractions and the drawing is in the generator's own units, so both
    // go through the viewBox size the generator writes into the markup.
    const viewBoxSize = Number(svgSource.match(/viewBox="0 0 (\d+)/)[1])
    const markWidth = markBounds.width * viewBoxSize
    const markHeight = markBounds.height * viewBoxSize
    const croppedSide = Math.max(markWidth, markHeight)
    const croppedLeft = markBounds.left * viewBoxSize - (croppedSide - markWidth) / 2
    const croppedTop = markBounds.top * viewBoxSize - (croppedSide - markHeight) / 2
    const round = (value) => Math.round(value * 1000) / 1000

    return svgSource
        .replace(`viewBox="0 0 ${viewBoxSize} ${viewBoxSize}"`, `viewBox="${round(croppedLeft)} ${round(croppedTop)} ${round(croppedSide)} ${round(croppedSide)}"`)
}

/**
 * The mark as a picture, drawn large at the weight its viewing size calls for and then resampled
 * down to that size. Drawing large and shrinking is what keeps the edges clean; asking the SVG
 * renderer for the small size directly is what made these scratchy.
 */
const getResampledMarkPng = async ({ sidePx, displaySidePx, strokeWeightMultiplier }) => {
    const largeMarkPng = await sharp(Buffer.from(renderMasterMarkSvg({ displaySidePx, strokeWeightMultiplier }))).png().toBuffer()

    return sharp(largeMarkPng).resize(sidePx, sidePx, { kernel: 'lanczos3' }).png().toBuffer()
}

/**
 * One square icon: the mark sitting in the middle of a background of the given colour. Leave the
 * colour out for a see-through icon, and give a corner fraction to round it off.
 */
const getIconPng = async ({ sidePx, displaySidePx, backgroundColor, markFraction, cornerFraction, strokeWeightMultiplier }) => {
    const markPng = await getResampledMarkPng({ sidePx: Math.round(sidePx * markFraction), displaySidePx, strokeWeightMultiplier })
    if (backgroundColor === undefined) {
        return sharp({ create: { width: sidePx, height: sidePx, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
            .composite([{ input: markPng, gravity: 'centre' }])
            .png()
            .toBuffer()
    }

    const cornerRadius = Math.round(sidePx * (cornerFraction ?? 0))
    const backgroundSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="${sidePx}" height="${sidePx}">`
        + `<rect width="${sidePx}" height="${sidePx}" rx="${cornerRadius}" ry="${cornerRadius}" fill="${backgroundColor}" />`
        + `</svg>`

    return sharp(Buffer.from(backgroundSvg))
        .composite([{ input: markPng, gravity: 'centre' }])
        .png()
        .toBuffer()
}

const writeGeneratedFile = async (filePath, contents) => {
    await writeFile(filePath, contents)
    console.log(`wrote ${path.relative(repositoryDirectory, filePath)}`)
}

// -------------------------- The favicon container --------------------------

/**
 * Packs the given PNGs into one .ico. Browsers pick whichever size they need out of it.
 *
 * The format is a six byte header, then one sixteen byte record per image, then the images
 * themselves. A width or height byte of zero means 256, which is why they are masked.
 */
const buildIcoFile = (pngOfSide) => {
    const header = Buffer.alloc(6)
    header.writeUInt16LE(0, 0)
    header.writeUInt16LE(1, 2)
    header.writeUInt16LE(pngOfSide.length, 4)

    let imageOffset = header.length + pngOfSide.length * 16
    const records = pngOfSide.map(({ side, png }) => {
        const record = Buffer.alloc(16)
        record.writeUInt8(side & 0xff, 0)
        record.writeUInt8(side & 0xff, 1)
        record.writeUInt8(0, 2)
        record.writeUInt8(0, 3)
        record.writeUInt16LE(1, 4)
        record.writeUInt16LE(32, 6)
        record.writeUInt32LE(png.length, 8)
        record.writeUInt32LE(imageOffset, 12)
        imageOffset += png.length
        return record
    })

    return Buffer.concat([header, ...records, ...pngOfSide.map(({ png }) => png)])
}

// -------------------------- Writing everything out --------------------------

const websiteAppDirectory = path.join(websiteDirectory, 'app')

// The tab icon, and the icon Android uses when the site is saved to a home screen. Drawn on
// nothing, so it sits on whatever colour the browser puts behind it. It is a big file but it is
// looked at around the size of a home screen icon, which is what sets its weight.
await writeGeneratedFile(
    path.join(websiteAppDirectory, 'icon.png'),
    await getIconPng({ sidePx: 512, displaySidePx: 192, markFraction: ICON_MARK_FRACTION }),
)

// iOS fills anything see-through with black, so this one gets a white ground instead.
await writeGeneratedFile(
    path.join(websiteAppDirectory, 'apple-icon.png'),
    await getIconPng({ sidePx: 180, displaySidePx: 180, backgroundColor: OPAQUE_ICON_BACKGROUND, markFraction: ICON_MARK_FRACTION }),
)

// The one place a mark is genuinely tiny. Each size in the file is drawn for itself, because the
// weight that carries at 48 pixels is a smudge at 16.
const faviconSides = [16, 32, 48]
await writeGeneratedFile(
    path.join(websiteAppDirectory, 'favicon.ico'),
    buildIcoFile(await Promise.all(faviconSides.map(async (side) => ({
        side,
        png: await getIconPng({ sidePx: side, displaySidePx: side, markFraction: FAVICON_MARK_FRACTION }),
    })))),
)

// The one image the macOS app's icons are built from. It lives here beside the script that
// draws it, and the macOS packaging script hands it to `cargo tauri icon`, which cuts it up into
// the .icns the macOS packager config references. It is drawn as a rounded square because it
// sits in a dock and a task bar next to other rounded squares.
await writeGeneratedFile(
    path.join(iconGenDirectory, 'icon-source.png'),
    await getIconPng({
        sidePx: 1024,
        displaySidePx: DESKTOP_ICON_DISPLAY_SIZE,
        backgroundColor: OPAQUE_ICON_BACKGROUND,
        markFraction: DESKTOP_ICON_MARK_FRACTION,
        cornerFraction: DESKTOP_ICON_CORNER_FRACTION,
        strokeWeightMultiplier: DESKTOP_ICON_STROKE_WEIGHT_MULTIPLIER,
    }),
)

// The Windows app icon. It goes in with the rest of the Windows packaging art rather than here
// beside the generator, because unlike icon-source.png above it is not a source image for something
// else to cut up — it is the finished file the packager and the client's build script both point at.
//
// It is written as a finished .ico for the reason the favicon is: a task bar icon is looked at at 16
// and 24 pixels, and one blind downsample of a 1024 pixel drawing turns the K's diagonals into a
// smudge at those sizes. Every size below is drawn for itself instead, at the weight its size calls
// for.
const windowsIconSides = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
await writeGeneratedFile(
    path.join(windowsPackagingAssetsDirectory, 'icon.ico'),
    buildIcoFile(await Promise.all(windowsIconSides.map(async (side) => ({
        side,
        png: await getIconPng({ sidePx: side, displaySidePx: side, markFraction: WINDOWS_ICON_MARK_FRACTION }),
    })))),
)
