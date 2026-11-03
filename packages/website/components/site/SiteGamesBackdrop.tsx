import { GameThumbnailImage } from '@/components/site/GameThumbnailImage'

export type SiteBackdropGame = {
    gameId: number
    name: string
    thumbnailUrl: string | null
}

/**
 * One arrangement of thumbnails, drawn on a canvas of a fixed size. Everything is measured in that
 * canvas's own pixels, from its top left corner, and the whole thing is then scaled as one piece
 * to fill whatever it is shown in. That is what keeps the arrangement itself fixed: a bigger
 * monitor gets the same picture larger rather than the same tiles spread further apart.
 */
type BackgroundScatter = {
    canvasWidth: number
    canvasHeight: number
    /**
     * How wide the canvas is ever allowed to be drawn, in real pixels. Past this it stops growing
     * and simply shows more of itself, because a picture that keeps scaling with the screen ends
     * up with the tiles too far apart to read as one scatter.
     */
    maxRenderedWidth: number
    tiles: { left: number, top: number, width: number, turn: number }[]
}

// Written out rather than worked out at random, because a random scatter would land somewhere
// different on the server than in the browser, and because these are placed to leave the middle
// of the canvas clear for the form.
const wideBackgroundScatter: BackgroundScatter = {
    canvasWidth: 1440,
    canvasHeight: 900,
    maxRenderedWidth: 1600,
    tiles: [
        { left: 58, top: 72, width: 144, turn: -8 },
        { left: 230, top: 306, width: 112, turn: 6 },
        { left: 29, top: 558, width: 176, turn: 4 },
        { left: 202, top: 756, width: 128, turn: -11 },
        { left: 288, top: 468, width: 160, turn: -6 },
        { left: 392, top: 196, width: 120, turn: 7 },
        { left: 389, top: 36, width: 112, turn: 9 },
        { left: 432, top: 648, width: 144, turn: -5 },
        { left: 590, top: 792, width: 112, turn: 7 },
        { left: 634, top: -18, width: 144, turn: -6 },
        { left: 835, top: 702, width: 128, turn: 10 },
        { left: 893, top: 54, width: 112, turn: -9 },
        { left: 1024, top: 300, width: 128, turn: 5 },
        { left: 1030, top: 566, width: 112, turn: -5 },
        { left: 1094, top: 738, width: 128, turn: -7 },
        { left: 1178, top: 126, width: 144, turn: 8 },
        { left: 1200, top: 372, width: 160, turn: 6 },
        { left: 1254, top: 578, width: 120, turn: -4 },
    ],
}

// A phone is a fifth of the width and gets its own arrangement rather than the wide one squashed.
// It has far less room to play with: the sidebar covers the first fifth of the screen, the header
// covers the top of it, and the form fills nearly all of what is left across the middle. So these
// sit in the bands above and below the form, with two showing as slivers either side of it. The
// three along the top are stepped, with the middle one dropped furthest, so the row reads as a
// scatter instead of a shelf pushed up under the header.
const narrowBackgroundScatter: BackgroundScatter = {
    canvasWidth: 390,
    canvasHeight: 844,
    // The widest screen that ever uses this one, since anything wider takes the arrangement above.
    maxRenderedWidth: 768,
    tiles: [
        { left: 98, top: 101, width: 80, turn: -9 },
        { left: 215, top: 143, width: 72, turn: 7 },
        { left: 328, top: 84, width: 80, turn: -5 },
        { left: 47, top: 490, width: 72, turn: 5 },
        { left: 351, top: 354, width: 72, turn: 6 },
        { left: 94, top: 701, width: 80, turn: 8 },
        { left: 218, top: 734, width: 80, turn: -6 },
        { left: 328, top: 692, width: 72, turn: 4 },
    ],
}

const BackgroundTiles = ({ games, scatter }: {
    games: SiteBackdropGame[]
    scatter: BackgroundScatter
}) => {
    // One canvas pixel, in whatever the screen is. It is the larger of the two ratios, so the
    // canvas always covers the screen and the overflow is cropped rather than showing a gap, and
    // it stops at the width the canvas is allowed to reach. Both ratios are given in viewport
    // units, which is also what makes zooming leave the picture alone: they and everything drawn
    // from them move together.
    const canvasPixel = `min(max(${(100 / scatter.canvasWidth).toFixed(5)}vw, ${(100 / scatter.canvasHeight).toFixed(5)}vh), ${(scatter.maxRenderedWidth / scatter.canvasWidth).toFixed(5)}px)`
    const inCanvasPixels = (length: number) => `${canvasPixel} * ${length}`

    return scatter.tiles.map((tile, tileIndex) => {
        const game = games[tileIndex % games.length]

        return (
            <div
                key={tileIndex}
                // TODO: AI-PICKED-VALUE: Each tile is faded to 45% and blurred by 2px, which pushes the games back behind the
                // card so the form reads first, without washing out the sky behind them.
                className="absolute overflow-hidden rounded-[10px] opacity-45 blur-[2px] shadow-[0_16px_40px_-28px_rgba(17,24,39,0.5)]"
                style={{
                    // Measured out from the middle, so what gets cropped is shared evenly between
                    // the two sides rather than all coming off the right and the bottom.
                    // Keep the original square's height and center, expanding its width to the shared ratio.
                    // The splash now uses the original square dimensions again, without that width expansion.
                    left: `calc(50% + ${inCanvasPixels(tile.left + tile.width / 2 - scatter.canvasWidth / 2)})`,
                    top: `calc(50% + ${inCanvasPixels(tile.top + tile.width / 2 - scatter.canvasHeight / 2)})`,
                    width: `calc(${inCanvasPixels(tile.width)})`,
                    aspectRatio: '1',
                    transform: `translate(-50%, -50%) rotate(${tile.turn}deg)`,
                }}
            >
                <GameThumbnailImage
                    name={game.name}
                    thumbnailUrl={game.thumbnailUrl}
                    // TODO: AI-PICKED-VALUE: These image sizes cover the largest enlarged tile in each capped canvas.
                    // TODO: AI-PICKED-VALUE: Square tiles cap at about 158px on mobile and 196px on desktop, rounded up here.
                    sizes="(max-width: 767px) 10rem, 13rem"
                    className="bg-gradient-to-br from-gray-100 to-gray-200"
                    isHighPriority={false}
                />
            </div>
        )
    })
}

/**
 * The games people are playing, scattered behind a card. There are usually far fewer
 * games than there are places to put one, so the same handful goes round the list again; at this
 * size and this far faded back they read as texture rather than as a gallery you could count.
 *
 * There are two arrangements, one for a wide screen and one for a phone, and the screen picks
 * between them. Sharing one would mean either tiles the size of the form on a phone or most of
 * the picture cropped away on a desktop.
 */
export const SiteGamesBackdrop = ({ games }: { games: SiteBackdropGame[] }) => {
    if (games.length === 0) return null

    return (
        // Held inside the screen it backs rather than pinned to the window. Pinned, it stayed put
        // while the page moved under it, so overscrolling past the bottom slid the footer up off
        // it and showed the games underneath. It also reaches back over the sidebar, because the
        // page it sits in starts to the right of that and the scatter should not.
        <div aria-hidden={true} className="site-games-backdrop pointer-events-none absolute overflow-hidden">
            <div className="hidden md:block">
                <BackgroundTiles games={games} scatter={wideBackgroundScatter} />
            </div>
            <div className="md:hidden">
                <BackgroundTiles games={games} scatter={narrowBackgroundScatter} />
            </div>
            {/* The form needs a clear ground to sit on, so the page colour is washed over the
                middle of the screen and fades out well before the edges. Everywhere else the
                games are left alone, which is the only reason they are visible at all. It is kept
                tight enough that it clears the card and little more, so it reads as ground under
                the form rather than as a glare over the middle of the page. */}
            <div className="absolute inset-0 bg-[radial-gradient(ellipse_34%_32%_at_center,var(--background)_0%,color-mix(in_srgb,var(--background)_45%,transparent)_50%,transparent_78%)]" />
        </div>
    )
}
