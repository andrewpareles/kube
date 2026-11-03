export const GAME_ROW_MAX_COLUMN_COUNT = 4

// The approved layout has four flexible landscape cards inside its 1248px content area.
// With padding outside the grid tracks, each image fills its column and the existing 30px gaps stay visible.
const GAME_ROW_GAP_WIDTH_PX = 30
const GAME_ROW_CARD_WIDTH_PX = 289.5

export const GAME_THUMBNAIL_SIZES = '(max-width: 560px) calc((100vw - 72px) / 2),(max-width: 820px) calc((100vw - 86px) / 2), (max-width: 1100px) calc((100vw - 140px) / 3), (max-width: 1328px) calc((100vw - 170px) / 4), 290px'

export const GAME_ROW_MAX_WIDTH_PX = GAME_ROW_CARD_WIDTH_PX * GAME_ROW_MAX_COLUMN_COUNT + GAME_ROW_GAP_WIDTH_PX * (GAME_ROW_MAX_COLUMN_COUNT - 1)
