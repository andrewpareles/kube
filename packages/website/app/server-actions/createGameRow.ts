import 'server-only'

import { count, eq } from 'drizzle-orm'
import { z } from 'zod'
import type { UIMessage } from 'ai'
import { db } from '@/lib/db'
import { chatMessages, games, userProfiles } from '@/supabase/migrations/schema'

// How many games one person can have. This is a stop on a script that has got away from someone
// rather than a number anybody should ever meet by hand.
const MAX_GAMES_PER_CREATOR = 5000

/**
 * Makes a game row. Every way of making a game goes through here, so the cap below cannot be got
 * around by making a game some other way.
 *
 * A new game has no world yet. The first one is written when its creator publishes from the editor.
 */
export const createGameRow = async ({
    creatorId,
    name,
    description,
    suggestedMinPlayers,
    maxPlayers,
    isPublished,
    initialMessage,
}: {
    creatorId: string
    name: string
    description: string | null
    suggestedMinPlayers: number
    maxPlayers: number
    isPublished: boolean
    initialMessage: string | null
}): Promise<number> => {
    const text = z.string().trim().min(1).max(100_000).nullable().parse(initialMessage)
    return db.transaction(async (tx) => {
        // Hold the creator's own row while we count and insert. Counting on its own is not enough:
        // two requests at once would both read the same number and both be let through, which is
        // exactly what a runaway script does. Taking the row first puts them in a queue instead.
        await tx
            .select({ id: userProfiles.id })
            .from(userProfiles)
            .where(eq(userProfiles.id, creatorId))
            .for('update')

        const [existingGames] = await tx
            .select({ games: count(games.gameId) })
            .from(games)
            .where(eq(games.creatorId, creatorId))

        if (existingGames.games >= MAX_GAMES_PER_CREATOR) {
            throw new Error(`You have reached the limit of ${MAX_GAMES_PER_CREATOR} games.`)
        }

        const [createdGame] = await tx
            .insert(games)
            .values({
                name,
                description,
                creatorId,
                suggestedMinPlayers,
                maxPlayers,
                isPublished,
            })
            .returning({
                gameId: games.gameId,
            })

        if (!createdGame) {
            throw new Error('Failed to create game.')
        }

        // Save the homepage prompt in the same transaction so the game and its first message exist together.
        if (text !== null) {
            const message: UIMessage = { id: crypto.randomUUID(), role: 'user', parts: [{ type: 'text', text }] }
            await tx.insert(chatMessages).values({ gameId: createdGame.gameId, seq: 1, message })
        }

        return createdGame.gameId
    })
}
