import { sql } from '@/lib/db'

// The page and startup route share the same rule: the creator's login, this game's current invitation, or a site admin's login.
export const getEditorAccess = async (gameId: number, profileId: string | null, inviteToken: string | null) => {
    const [game] = await sql<{ name: string, description: string | null, suggestedMinPlayers: number, maxPlayers: number, creatorId: string, isCreator: boolean, isPublished: boolean, isPublic: boolean, thumbnailUuid: string | null }[]>`
        select g.name, g.description, g.suggested_min_players as "suggestedMinPlayers", g.max_players as "maxPlayers", g.creator_id::text as "creatorId", coalesce(g.creator_id::text=${profileId}, false) as "isCreator", coalesce(g.is_published, false) as "isPublished", coalesce(g.ai_thumbnail_uuid, g.screenshot_uuid)::text as "thumbnailUuid",
            coalesce(g.is_published, false) and exists (select 1 from game_worlds w where w.game_id=g.game_id) as "isPublic"
        from game_metadata g left join editor_share_auth s on s.game_id=g.game_id
        where g.game_id=${gameId} and (
            g.creator_id::text=${profileId} or (s.invite_token=${inviteToken} and s.invite_token <> '')
            or exists (select 1 from site_admins a where a.profile_id::text=${profileId})
        )
    `
    return game
}
