import 'server-only'

import { and, count, desc, eq, inArray, isNotNull, sql } from 'drizzle-orm'
import { db } from '@/lib/db'
import { hardcodedBackdropGames, SHOULD_USE_HARDCODED_BACKDROP_GAMES } from '@/lib/siteBackdropGames'
import { getUser } from '@/lib/siteUser'
import { getPlayerLook, type PlayerLook } from '@/lib/siteAvatar'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'
import { gameWorlds, games, runningPlayerSessions, runningServers, siteAdmins, userFavorites, userLikes, userProfiles } from '@/supabase/migrations/schema'

type BaseGameRow = {
    gameId: number
    name: string
    thumbnailUuid: string | null
    createdAt: string | null
    creatorId: string
    creatorUserId: number
    creatorUsername: string
    isPublished: boolean | null
    totalVisits: number
    suggestedMinPlayers: number
    maxPlayers: number
}

export type SiteGameSummary = {
    gameId: number
    name: string
    thumbnailUuid: string | null
    createdAt: string | null
    creatorId: string
    creatorUserId: number
    creatorUsername: string
    isPublished: boolean
    totalVisits: number
    suggestedMinPlayers: number
    maxPlayers: number
    concurrentPlayers: number
    likes: number
}

export type ProfileGamesResult = {
    isOwner: boolean
    username: string
    createdAt: string | null
    games: SiteGameSummary[]
}

export type SiteGameDetail = {
    gameId: number
    name: string
    description: string | null
    aiThumbnailUuid: string | null
    screenshotUuid: string | null
    creatorId: string
    creatorUserId: number
    creatorUsername: string
    createdAt: string | null
    isPublished: boolean
    totalVisits: number
    suggestedMinPlayers: number
    maxPlayers: number
    isLikedByViewer: boolean
    isFavoritedByViewer: boolean
    isOwner: boolean
    hasWorld: boolean // if there is at least one corresponding entry in the game_worlds table
}

export type SiteGameServer = {
    requestId: string
    maxPlayers: number
    players: SiteGameServerPlayer[]
}

export type SiteGameServerPlayer = {
    userId: number
    username: string
    look: PlayerLook
}

const getConcurrentPlayersOfGameId = async ({
    gameIds,
}: {
    gameIds: number[]
}) => {
    const concurrentPlayersOfGameId: { [gameId: number]: number } = {}
    if (gameIds.length === 0) {
        return concurrentPlayersOfGameId
    }

    const activePlayerRows = await db
        .select({
            gameId: runningServers.gameId,
            concurrentPlayers: count(runningPlayerSessions.profileId),
        })
        .from(runningServers)
        // Reserved-but-not-yet-connected players are joining, not playing, so they are joined away
        // here rather than filtered in the where clause - that keeps this a left join, so a server
        // with nobody connected still counts as zero instead of dropping out of the results.
        .leftJoin(
            runningPlayerSessions,
            and(
                eq(runningPlayerSessions.requestId, runningServers.requestId),
                isNotNull(runningPlayerSessions.connectedAt),
            ),
        )
        .where(inArray(runningServers.gameId, gameIds))
        .groupBy(runningServers.gameId)

    for (const activePlayerRow of activePlayerRows) {
        concurrentPlayersOfGameId[activePlayerRow.gameId] = activePlayerRow.concurrentPlayers
    }

    return concurrentPlayersOfGameId
}

const getLikesOfGameId = async ({
    gameIds,
}: {
    gameIds: number[]
}) => {
    const likesOfGameId: { [gameId: number]: number } = {}
    if (gameIds.length === 0) {
        return likesOfGameId
    }

    const likeRows = await db
        .select({
            gameId: userLikes.gameId,
            likes: count(userLikes.profileId),
        })
        .from(userLikes)
        .where(inArray(userLikes.gameId, gameIds))
        .groupBy(userLikes.gameId)

    for (const likeRow of likeRows) {
        likesOfGameId[likeRow.gameId] = likeRow.likes
    }

    return likesOfGameId
}

const getGamesWithStats = async ({
    gameRows,
}: {
    gameRows: BaseGameRow[]
}): Promise<SiteGameSummary[]> => {
    const gameIds = gameRows.map((gameRow) => gameRow.gameId)
    const [concurrentPlayersOfGameId, likesOfGameId] = await Promise.all([
        getConcurrentPlayersOfGameId({ gameIds }),
        getLikesOfGameId({ gameIds }),
    ])

    return gameRows.map((gameRow) => ({
        gameId: gameRow.gameId,
        name: gameRow.name,
        thumbnailUuid: gameRow.thumbnailUuid,
        createdAt: gameRow.createdAt,
        creatorId: gameRow.creatorId,
        creatorUserId: gameRow.creatorUserId,
        creatorUsername: gameRow.creatorUsername,
        isPublished: gameRow.isPublished ?? false,
        totalVisits: gameRow.totalVisits,
        suggestedMinPlayers: gameRow.suggestedMinPlayers,
        maxPlayers: gameRow.maxPlayers,
        concurrentPlayers: concurrentPlayersOfGameId[gameRow.gameId] ?? 0,
        likes: likesOfGameId[gameRow.gameId] ?? 0,
    }))
}

// The picture a game is shown with: its AI thumbnail, or its newest screenshot when it has none.
export const gameThumbnailUuid = sql<string | null>`coalesce(${games.aiThumbnailUuid}, ${games.screenshotUuid})`

// Everything a game row needs before its stats are attached. Shared so the queries below cannot
// drift into selecting different columns and handing getGamesWithStats a different shape.
const gameRowColumns = {
    gameId: games.gameId,
    name: games.name,
    thumbnailUuid: gameThumbnailUuid,
    createdAt: games.createdAt,
    creatorId: games.creatorId,
    creatorUserId: userProfiles.userId,
    creatorUsername: userProfiles.username,
    isPublished: games.isPublished,
    totalVisits: games.totalVisits,
    suggestedMinPlayers: games.suggestedMinPlayers,
    maxPlayers: games.maxPlayers,
}

// Whether a game has a world saved for it. Making a game writes no world: the first one is written
// when its creator first opens the editor on it. Until then there is nothing to play, so the game is
// only its creator's own draft and stays off every list of other people's games.
const hasWorld = sql<boolean>`exists (select 1 from ${gameWorlds} where ${gameWorlds.gameId} = ${games.gameId})`

// How many people have liked each game, as something a query can join to and order by. Games nobody
// has liked are missing from it rather than sitting there as a zero, which is why the ordering below
// has to coalesce: on a descending sort Postgres puts nulls first, so unliked games would come top.
const likeCountsOfGame = db
    .select({
        gameId: userLikes.gameId,
        likes: count(userLikes.profileId).as('likes'),
    })
    .from(userLikes)
    .groupBy(userLikes.gameId)
    .as('like_counts')

const playerCountsOfGame = db
    .select({
        gameId: runningServers.gameId,
        players: count(runningPlayerSessions.profileId).as('players'),
    })
    .from(runningServers)
    .innerJoin(
        runningPlayerSessions,
        and(
            eq(runningPlayerSessions.requestId, runningServers.requestId),
            isNotNull(runningPlayerSessions.connectedAt),
        ),
    )
    .groupBy(runningServers.gameId)
    .as('player_counts')

// TODO: AI-PICKED-VALUE: Each list stops at 1000 games, so publishing a flood of games cannot make every page load read them all.
const MAX_LISTED_GAMES = 1000

/**
 * Published games, most players right now first, then most liked, with newest breaking a tie so two
 * games on the same counts never swap places between one page load and the next.
 */
export const getTopGames = async (): Promise<SiteGameSummary[]> => {
    const gameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .leftJoin(playerCountsOfGame, eq(playerCountsOfGame.gameId, games.gameId))
        .leftJoin(likeCountsOfGame, eq(likeCountsOfGame.gameId, games.gameId))
        .where(and(eq(games.isPublished, true), hasWorld))
        .orderBy(desc(sql`coalesce(${playerCountsOfGame.players}, 0)`), desc(sql`coalesce(${likeCountsOfGame.likes}, 0)`), desc(games.createdAt))
        .limit(MAX_LISTED_GAMES)

    return getGamesWithStats({
        gameRows,
    })
}

// Staff Picks on the games page, in the order they show.
const STAFF_PICK_GAME_IDS = [393, 256, 328, 323, 334]

/** The staff picks that are published, in the order they were picked. */
export const getStaffPickGames = async (): Promise<SiteGameSummary[]> => {
    const gameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .where(and(inArray(games.gameId, STAFF_PICK_GAME_IDS), eq(games.isPublished, true), hasWorld))

    return getGamesWithStats({
        gameRows: gameRows.sort((a, b) => STAFF_PICK_GAME_IDS.indexOf(a.gameId) - STAFF_PICK_GAME_IDS.indexOf(b.gameId)),
    })
}

/** Published games, newest first. */
export const getRecentGames = async (): Promise<SiteGameSummary[]> => {
    const gameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .where(and(eq(games.isPublished, true), hasWorld))
        .orderBy(desc(games.createdAt))
        .limit(MAX_LISTED_GAMES)

    return getGamesWithStats({
        gameRows,
    })
}

/** The games shown scattered behind a SiteBackdropCard, most liked first. */
export const getBackdropGames = async () => {
    // The fixed background needs no game queries; both modes use the same thumbnail layout.
    if (SHOULD_USE_HARDCODED_BACKDROP_GAMES) return hardcodedBackdropGames

    const topGames = await getTopGames()
    return topGames.map((game) => ({
        gameId: game.gameId,
        name: game.name,
        thumbnailUrl: urlOfGameThumbnail(game.thumbnailUuid),
    }))
}

/** Every game the signed-in user made, newest first, or nothing at all when nobody is signed in. */
export const getMyGames = async (): Promise<SiteGameSummary[]> => {
    const user = await getUser()
    if (user === null) {
        return []
    }

    const gameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .where(eq(games.creatorId, user.id))
        .orderBy(desc(games.createdAt))

    return getGamesWithStats({
        gameRows,
    })
}

/** Every game anyone has made, private ones too, newest first. Only site admins get them; everyone else gets null. */
export const getAllGamesIfAdmin = async (): Promise<SiteGameSummary[] | null> => {
    const user = await getUser()
    if (user === null) {
        return null
    }

    const [admin] = await db
        .select({ profileId: siteAdmins.profileId })
        .from(siteAdmins)
        .where(eq(siteAdmins.profileId, user.id))
        .limit(1)
    if (!admin) {
        return null
    }

    const gameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .orderBy(desc(games.createdAt))

    return getGamesWithStats({
        gameRows,
    })
}

// games that the user is allowed to access on the profile
export const getAllowedProfileGames = async (userId: number): Promise<ProfileGamesResult | null> => {
    const user = await getUser()
    const [profile] = await db
        .select({
            id: userProfiles.id,
            username: userProfiles.username,
            createdAt: userProfiles.createdAt,
        })
        .from(userProfiles)
        .where(eq(userProfiles.userId, userId))
        .limit(1)

    if (!profile) {
        return null
    }

    const isOwner = user?.id === profile.id

    const allowedGamesWhere = isOwner
        ? eq(games.creatorId, profile.id)
        : and(
            eq(games.creatorId, profile.id),
            eq(games.isPublished, true),
            hasWorld,
        )

    const allowedGameRows = await db
        .select(gameRowColumns)
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .where(allowedGamesWhere)
        .orderBy(desc(games.createdAt))

    return {
        isOwner,
        username: profile.username,
        createdAt: profile.createdAt,
        games: await getGamesWithStats({ gameRows: allowedGameRows }),
    }
}

export const getGameDetailIfAllowed = async (gameId: number): Promise<SiteGameDetail | null> => {
    const user = await getUser()
    const [game] = await db
        .select({
            gameId: games.gameId,
            name: games.name,
            description: games.description,
            aiThumbnailUuid: games.aiThumbnailUuid,
            screenshotUuid: games.screenshotUuid,
            creatorId: games.creatorId,
            creatorUserId: userProfiles.userId,
            creatorUsername: userProfiles.username,
            createdAt: games.createdAt,
            isPublished: games.isPublished,
            totalVisits: games.totalVisits,
            suggestedMinPlayers: games.suggestedMinPlayers,
            maxPlayers: games.maxPlayers,
            hasWorld,
        })
        .from(games)
        .innerJoin(userProfiles, eq(games.creatorId, userProfiles.id))
        .where(eq(games.gameId, gameId))
        .limit(1)

    if (!game) {
        return null
    }

    const isPublished = game.isPublished ?? false
    const isOwner = user?.id === game.creatorId
    // A private game, or one whose creator has never saved a world for it, is only its creator's to see.
    if ((!isPublished || !game.hasWorld) && !isOwner) {
        return null
    }

    let isLikedByViewer = false
    let isFavoritedByViewer = false
    if (user) {
        const [viewerLike] = await db
            .select({
                profileId: userLikes.profileId,
            })
            .from(userLikes)
            .where(
                and(
                    eq(userLikes.gameId, gameId),
                    eq(userLikes.profileId, user.id),
                ),
            )
            .limit(1)

        isLikedByViewer = Boolean(viewerLike)

        const [viewerFavorite] = await db
            .select({
                profileId: userFavorites.profileId,
            })
            .from(userFavorites)
            .where(
                and(
                    eq(userFavorites.gameId, gameId),
                    eq(userFavorites.profileId, user.id),
                ),
            )
            .limit(1)

        isFavoritedByViewer = Boolean(viewerFavorite)
    }

    return {
        gameId: game.gameId,
        name: game.name,
        description: game.description,
        aiThumbnailUuid: game.aiThumbnailUuid,
        screenshotUuid: game.screenshotUuid,
        creatorId: game.creatorId,
        creatorUserId: game.creatorUserId,
        creatorUsername: game.creatorUsername,
        createdAt: game.createdAt,
        isPublished,
        totalVisits: game.totalVisits,
        suggestedMinPlayers: game.suggestedMinPlayers,
        maxPlayers: game.maxPlayers,
        isLikedByViewer,
        isFavoritedByViewer,
        isOwner,
        hasWorld: game.hasWorld,
    }
}

export const getPublishedGameServers = async (gameId: number): Promise<SiteGameServer[]> => {
    const [publishedGame] = await db
        .select({
            gameId: games.gameId,
        })
        .from(games)
        .where(
            and(
                eq(games.gameId, gameId),
                eq(games.isPublished, true),
            ),
        )
        .limit(1)

    if (!publishedGame) {
        return []
    }
    const [latestGameworld] = await db
        .select({ versionNum: gameWorlds.versionNum })
        .from(gameWorlds)
        .where(eq(gameWorlds.gameId, gameId))
        .orderBy(desc(gameWorlds.versionNum))
        .limit(1)

    if (!latestGameworld) {
        return []
    }

    const serverRows = await db
        .select({
            requestId: runningServers.requestId,
            maxPlayers: runningServers.maxPlayers,
            userId: userProfiles.userId,
            username: userProfiles.username,
        })
        .from(runningServers)
        // Only players the game server has stamped as connected are really in the server. Left
        // joining on that instead of filtering on it keeps empty servers in the list so they can
        // still be joined.
        .leftJoin(
            runningPlayerSessions,
            and(
                eq(runningPlayerSessions.requestId, runningServers.requestId),
                isNotNull(runningPlayerSessions.connectedAt),
            ),
        )
        .leftJoin(userProfiles, eq(runningPlayerSessions.profileId, userProfiles.id))
        .where(
            and(
                eq(runningServers.gameId, gameId),
                eq(runningServers.gameworldVersionNum, latestGameworld.versionNum),
            ),
        )
        .orderBy(desc(runningServers.maxPlayers))

    // Every player in a server shows their avatar, so each one's look is read once.
    const playerUserIds = [...new Set(serverRows.flatMap((serverRow) => serverRow.userId === null ? [] : [serverRow.userId]))]
    const looks = await Promise.all(playerUserIds.map((userId) => getPlayerLook(userId)))
    const lookOfUserId: { [userId: number]: PlayerLook } = {}
    playerUserIds.forEach((userId, index) => { lookOfUserId[userId] = looks[index] })

    const serverOfRequestId: {
        [requestId: string]: SiteGameServer
    } = {}

    for (const serverRow of serverRows) {
        if (!serverOfRequestId[serverRow.requestId]) {
            serverOfRequestId[serverRow.requestId] = {
                requestId: serverRow.requestId,
                maxPlayers: serverRow.maxPlayers,
                players: [],
            }
        }
        if (serverRow.userId !== null && serverRow.username !== null) {
            serverOfRequestId[serverRow.requestId].players.push({
                userId: serverRow.userId,
                username: serverRow.username,
                look: lookOfUserId[serverRow.userId],
            })
        }
    }

    return Object.values(serverOfRequestId)
        .sort((serverA, serverB) => {
            if (serverA.players.length !== serverB.players.length) {
                return serverB.players.length - serverA.players.length
            }
            return serverA.requestId.localeCompare(serverB.requestId)
        })
}
