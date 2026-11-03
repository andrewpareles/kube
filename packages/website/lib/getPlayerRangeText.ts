// A game's player count, like "2-8 players": its suggested min players up to its max players. The suggested min never
// shows above max players.
export const getPlayerRangeText = ({ suggestedMinPlayers, maxPlayers }: { suggestedMinPlayers: number, maxPlayers: number }) => {
    const shownSuggestedMinPlayers = Math.min(suggestedMinPlayers, maxPlayers)
    if (shownSuggestedMinPlayers === maxPlayers) return `${maxPlayers} ${maxPlayers === 1 ? 'player' : 'players'}`
    return `${shownSuggestedMinPlayers}-${maxPlayers} players`
}
