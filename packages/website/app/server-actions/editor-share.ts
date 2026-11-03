'use server'

import { randomBytes } from 'node:crypto'
import { sql } from 'drizzle-orm'
import { z } from 'zod'
import { editorShareAuth, games } from '@/supabase/migrations/schema'
import { db } from '@/lib/db'
import { getUser } from '@/lib/siteUser'

const shareRequestSchema = z.object({ gameId: z.number().int().positive().safe(), action: z.enum(['create', 'hide']) })

// Only the logged-in creator can create or hide the persistent invitation.
export const updateEditorShare = async (gameId: number, action: 'create' | 'hide'): Promise<{ inviteToken: string | null } | { error: string }> => {
    const user = await getUser()
    if (user === null) return { error: 'Please log in again.' }
    const parsed = shareRequestSchema.safeParse({ gameId, action })
    if (!parsed.success) return { error: 'Invalid sharing request.' }

    // TODO: AI-PICKED-VALUE: Use 32 random bytes for an unguessable, URL-safe invitation.
    const inviteToken = action === 'hide' ? null : randomBytes(32).toString('base64url')
    // One statement checks ownership and serializes changes to this game's invitation.
    // Opening Share preserves an existing link; Hide clears it.
    const [share] = await db.insert(editorShareAuth)
        .select(sql`select ${games.gameId}, ${inviteToken} from ${games} where ${games.gameId}=${gameId} and ${games.creatorId}=${user.id}`)
        .onConflictDoUpdate({
            target: editorShareAuth.gameId,
            set: { inviteToken: action === 'create' ? sql`coalesce(${editorShareAuth.inviteToken}, excluded.invite_token)` : null },
        })
        .returning({ inviteToken: editorShareAuth.inviteToken })
    if (!share) return { error: 'Game not found.' }
    return share
}
