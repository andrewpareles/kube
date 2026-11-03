// Draws the cursors built into the game into this folder: arrow.png, pointer.png and crosshair.png, at 8 pixels per point.
// The arrow and hand are Font Awesome Free's arrow-pointer and hand-pointer (see SOURCES.md). All three are drawn like the
// kube.new logo. Every shape in a cursor has one outline width. It prints each
// cursor's point that clicks, which crates/client/src/play_runner/mouse_cursor.rs keeps.
// Run from the repo root: node assets/cursors/draw_cursors.mjs
import { createRequire } from 'node:module'

const sharp = createRequire(new URL('../../packages/website/package.json', import.meta.url))('sharp')
const outDir = new URL('.', import.meta.url).pathname

const PIXELS_PER_POINT = 8

// The kube.new look is a lavender face, a violet-to-blue outline and the logo's glow.
const KUBE_LOOK = { faceColor: '#f4f0ff', outlinePoints: 1.6, outlineGradientColors: ['#4c00ff', '#33a7ff'], glowColor: '#336dff', glowRadiusPoints: 0.25 }

// Font Awesome Free 7.3.1: arrow-pointer, and the outer edge of hand-pointer (solid). Both are 448 x 512.
const ARROW_PATH = 'M77.3 2.5c8.1-4.1 17.9-3.2 25.1 2.3l320 239.9c8.3 6.2 11.6 17 8.4 26.8s-12.4 16.4-22.8 16.4l-152.3 0 88.9 177.7c7.9 15.8 1.5 35-14.3 42.9s-35 1.5-42.9-14.3l-88.9-177.7-91.3 121.8c-6.2 8.3-17 11.6-26.8 8.4S64 434.3 64 424L64 24c0-9.1 5.1-17.4 13.3-21.5z'
const HAND_PATH = 'M128 40c0-22.1 17.9-40 40-40s40 17.9 40 40l0 148.2c8.5-7.6 19.7-12.2 32-12.2 20.6 0 38.2 13 45 31.2 8.8-9.3 21.2-15.2 35-15.2 25.3 0 46 19.5 47.9 44.3 8.5-7.7 19.8-12.3 32.1-12.3 26.5 0 48 21.5 48 48l0 112c0 70.7-57.3 128-128 128l-85.3 0c-5 0-9.9-.3-14.7-1-55.3-5.6-106.2-34-140-79L8 336c-13.3-17.7-9.7-42.7 8-56s42.7-9.7 56 8l56 74.7 0-322.7z'

// A cursor is shapes in one coordinate space, none of them transformed, so a gradient runs across all of them. Every
// shape's outline is a stroke twice the outline width under all the faces, so touching shapes share one even outline.
// Lines go on top at the outline width.
const drawCursor = async ({ shapes, lines, unitsPerPoint, roughBox, look }) => {
  const outline = look.outlinePoints * unitsPerPoint
  const outlinePaint = look.outlineGradientColors ? 'url(#outline)' : look.outlineColor
  const svgFor = (box, pixelsPerUnit) => {
    const angle = 42 * Math.PI / 180
    const half = Math.hypot(box.width, box.height) / 2
    const [centerX, centerY] = [box.x + box.width / 2, box.y + box.height / 2]
    const glowRadius = (look.glowRadiusPoints ?? 0) * unitsPerPoint
    const gradient = look.outlineGradientColors
      ? `<linearGradient id="outline" gradientUnits="userSpaceOnUse" x1="${centerX - Math.cos(angle) * half}" y1="${centerY - Math.sin(angle) * half}" x2="${centerX + Math.cos(angle) * half}" y2="${centerY + Math.sin(angle) * half}"><stop offset="0" stop-color="${look.outlineGradientColors[0]}"/><stop offset="1" stop-color="${look.outlineGradientColors[1]}"/></linearGradient>`
      : ''
    const glow = look.glowColor
      ? `<filter id="glow" filterUnits="userSpaceOnUse" x="${box.x}" y="${box.y}" width="${box.width}" height="${box.height}" color-interpolation-filters="sRGB"><feGaussianBlur in="SourceAlpha" stdDeviation="${glowRadius}" result="outer-blur"/><feGaussianBlur in="SourceAlpha" stdDeviation="${glowRadius / 3}" result="inner-blur"/><feFlood flood-color="${look.glowColor}" flood-opacity="0.25" result="glow-color"/><feComposite in="glow-color" in2="outer-blur" operator="in" result="outer-glow"/><feComposite in="glow-color" in2="inner-blur" operator="in" result="inner-glow"/><feMerge><feMergeNode in="outer-glow"/><feMergeNode in="inner-glow"/><feMergeNode in="SourceGraphic"/></feMerge></filter>`
      : ''
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${box.x} ${box.y} ${box.width} ${box.height}" width="${box.width * pixelsPerUnit}" height="${box.height * pixelsPerUnit}"><defs>
      ${gradient}${glow}
    </defs><g${look.glowColor ? ' filter="url(#glow)"' : ''}>
      ${shapes.map((shape) => shape.outline(outline, outlinePaint)).join('')}
      ${shapes.map((shape) => shape.face(look.faceColor)).join('')}
      ${(lines ?? []).map((line) => line(`fill="none" stroke="${outlinePaint}" stroke-width="${outline}" stroke-linecap="round"`)).join('')}
    </g></svg>`
  }

  // Drawn big once to find the tight box and the topmost solid point, which is the tip that clicks.
  const measurePixelsPerUnit = 32 / unitsPerPoint
  const { data, info } = await sharp(Buffer.from(svgFor(roughBox, measurePixelsPerUnit))).raw().toBuffer({ resolveWithObject: true })
  let [minX, minY, maxX, maxY, tipY] = [Infinity, Infinity, -Infinity, -Infinity, Infinity]
  let tipXs = []
  for (let y = 0; y < info.height; y++) {
    for (let x = 0; x < info.width; x++) {
      const alpha = data[(y * info.width + x) * info.channels + 3]
      if (alpha > 4) {
        minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y)
      }
      if (alpha > 200 && y <= tipY) {
        if (y < tipY) tipXs = []
        tipY = y
        tipXs.push(x)
      }
    }
  }
  const toUnitsX = (x) => roughBox.x + x / measurePixelsPerUnit
  const toUnitsY = (y) => roughBox.y + y / measurePixelsPerUnit
  const box = { x: toUnitsX(minX), y: toUnitsY(minY), width: (maxX + 1 - minX) / measurePixelsPerUnit, height: (maxY + 1 - minY) / measurePixelsPerUnit }
  const tip = [toUnitsX(tipXs.reduce((sum, x) => sum + x, 0) / tipXs.length), toUnitsY(tipY)]
  return {
    svg: svgFor(box, PIXELS_PER_POINT / unitsPerPoint),
    hotspot: [(tip[0] - box.x) / box.width, (tip[1] - box.y) / box.height],
  }
}

const pathShape = (pathData) => ({
  outline: (outlineWidth, outlinePaint) => `<path d="${pathData}" fill="${outlinePaint}" stroke="${outlinePaint}" stroke-width="${outlineWidth * 2}" stroke-linejoin="round"/>`,
  face: (faceColor) => `<path d="${pathData}" fill="${faceColor}"/>`,
})
// A straight piece with round ends from one point to another, drawn as a line.
const capsuleShape = ([x1, y1], [x2, y2], width) => ({
  outline: (outlineWidth, outlinePaint) => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${outlinePaint}" stroke-width="${width + outlineWidth * 2}" stroke-linecap="round"/>`,
  face: (faceColor) => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${faceColor}" stroke-width="${width}" stroke-linecap="round"/>`,
})

// ------------------------------ arrow ------------------------------
// The stem runs along (88.9, 177.7) from its notches at (255.7, 287.9) and (198.5, 316.5), is 64 units wide, and its
// round end reaches (330.3, 508.5). A wider capsule along the same line, starting inside the head and reaching the same
// end, makes the stem wider.
const ARROW_HEIGHT_POINTS = 25
const STEM_WIDTH_UNITS = 86
const stemDirection = [88.9 / Math.hypot(88.9, 177.7), 177.7 / Math.hypot(88.9, 177.7)]
const stemStart = [227.1 - stemDirection[0] * 60, 302.2 - stemDirection[1] * 60]
const stemEnd = [330.3 - stemDirection[0] * STEM_WIDTH_UNITS / 2, 508.5 - stemDirection[1] * STEM_WIDTH_UNITS / 2]
const arrow = await drawCursor({
  shapes: [pathShape(ARROW_PATH), capsuleShape(stemStart, stemEnd, STEM_WIDTH_UNITS)],
  unitsPerPoint: 512 / ARROW_HEIGHT_POINTS,
  roughBox: { x: -40, y: -60, width: 528, height: 632 },
  look: KUBE_LOOK,
})

// ------------------------------ pointing hand ------------------------------
// The solid hand's finger creases are three slots centered at x 224, 288 and 352, straight from y 304 to 400. They're
// drawn as lines down the middle of each slot.
const HAND_HEIGHT_POINTS = 24
const pointer = await drawCursor({
  shapes: [pathShape(HAND_PATH)],
  lines: [224, 288, 352].map((centerX) => (attributes) => `<line x1="${centerX}" y1="304" x2="${centerX}" y2="400" ${attributes}/>`),
  unitsPerPoint: 512 / HAND_HEIGHT_POINTS,
  roughBox: { x: -60, y: -60, width: 568, height: 632 },
  look: KUBE_LOOK,
})

// ------------------------------ crosshair ------------------------------
// Four short lines around the point that clicks, in points.
const CROSSHAIR_GAP_POINTS = 6
const CROSSHAIR_LINE_POINTS = 12
const CROSSHAIR_LINE_WIDTH_POINTS = 2.5
const crosshairCenter = 32
const crosshair = await drawCursor({
  shapes: [[0, -1], [0, 1], [-1, 0], [1, 0]].map(([dx, dy]) => capsuleShape(
    [crosshairCenter + dx * (CROSSHAIR_GAP_POINTS + CROSSHAIR_LINE_WIDTH_POINTS / 2), crosshairCenter + dy * (CROSSHAIR_GAP_POINTS + CROSSHAIR_LINE_WIDTH_POINTS / 2)],
    [crosshairCenter + dx * (CROSSHAIR_GAP_POINTS + CROSSHAIR_LINE_POINTS - CROSSHAIR_LINE_WIDTH_POINTS / 2), crosshairCenter + dy * (CROSSHAIR_GAP_POINTS + CROSSHAIR_LINE_POINTS - CROSSHAIR_LINE_WIDTH_POINTS / 2)],
    CROSSHAIR_LINE_WIDTH_POINTS,
  )),
  unitsPerPoint: 1,
  roughBox: { x: 0, y: 0, width: 64, height: 64 },
  look: KUBE_LOOK,
})
crosshair.hotspot = [0.5, 0.5]

for (const [name, { svg, hotspot }] of Object.entries({ arrow, pointer, crosshair })) {
  await sharp(Buffer.from(svg)).png().toFile(`${outDir}${name}.png`)
  console.log(`${name}: hotspotX ${hotspot[0].toFixed(4)}, hotspotY ${hotspot[1].toFixed(4)}`)
}
