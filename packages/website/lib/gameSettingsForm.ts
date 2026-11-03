// What the new game form and the game settings form both hold while someone is typing, and the
// rules they are both held to. Max players stays a string here so the box can sit empty for a
// moment mid-edit instead of snapping back to a number under the cursor.
export type GameSettingsFormValues = {
    name: string
    description: string
    suggestedMinPlayersInput: string
    maxPlayersInput: string
    isPublished: boolean
}

export const MAX_PLAYERS_LIMIT = 40
export const NEW_GAME_DEFAULT_NAME = 'My Game'
export const NEW_GAME_DEFAULT_SUGGESTED_MIN_PLAYERS = 1
export const NEW_GAME_DEFAULT_MAX_PLAYERS = 8

// Manual creation starts with an empty title; the editor's automatic creation keeps its default name.
export const newGameFormValues = (): GameSettingsFormValues => ({
    name: '',
    description: '',
    suggestedMinPlayersInput: NEW_GAME_DEFAULT_SUGGESTED_MIN_PLAYERS.toString(),
    maxPlayersInput: NEW_GAME_DEFAULT_MAX_PLAYERS.toString(),
    isPublished: false,
})

/** The message to show under the form, or null when the values are ready to send. */
export const getGameSettingsFormError = ({ name, suggestedMinPlayersInput, maxPlayersInput }: GameSettingsFormValues): string | null => {
    if (!name.trim()) {
        return 'Game title is required.'
    }
    const maxPlayers = Number(maxPlayersInput)
    if (!Number.isInteger(maxPlayers) || maxPlayers < 1 || maxPlayers > MAX_PLAYERS_LIMIT) {
        return `Max players must be between 1 and ${MAX_PLAYERS_LIMIT}.`
    }
    const suggestedMinPlayers = Number(suggestedMinPlayersInput)
    if (!Number.isInteger(suggestedMinPlayers) || suggestedMinPlayers < 1 || suggestedMinPlayers > maxPlayers) {
        return 'Suggested min players must be between 1 and max players.'
    }
    return null
}
