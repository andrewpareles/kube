export const getMyGamesSplitByPublishState = <Game extends { isPublished: boolean }>(games: Game[]) => {
    const publishedGames: Game[] = []
    const privateGames: Game[] = []

    for (const game of games) {
        if (game.isPublished) {
            publishedGames.push(game)
        } else {
            privateGames.push(game)
        }
    }

    return {
        publishedGames,
        privateGames,
    }
}
