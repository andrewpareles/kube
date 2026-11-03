import { createMiddleware } from 'hono/factory'
import { HTTPException } from 'hono/http-exception'
import { z } from 'zod'
import { database } from './database.ts'
import { env } from './env.ts'

// Node authenticates chat directly now that the browser no longer goes through Rust.
export const requireEditorAccess = createMiddleware<{
    Variables: { profileId: string }
}>(async (c, next) => {
    const authorization = c.req.header('authorization')
    const inviteToken = authorization?.startsWith('Invite ') ? authorization.slice(7) : null
    let profileId: string | null = null
    if (!inviteToken) {
        if (!authorization?.startsWith('Bearer ') || authorization.length <= 7)
            throw new HTTPException(401, { message: 'Please log in or use an invite link.' })
        const response = await fetch(`${env.SUPABASE_URL}/auth/v1/user`, {
            headers: { apikey: env.SUPABASE_SECRET_KEY, Authorization: authorization },
            signal: AbortSignal.timeout(30_000),
        })
        if (!response.ok) throw new HTTPException(401, { message: 'Please log in again.' })
        profileId = z.object({ id: z.string() }).parse(await response.json()).id
    }
    // Check the current owner or invitation for this game on every request. All usage belongs to the game's creator,
    // including turns started by an invited guest.
    c.set('profileId', await database.browserAccess(profileId, inviteToken))
    await next()
})
