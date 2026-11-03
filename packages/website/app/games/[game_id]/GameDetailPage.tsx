'use client'

import Link from 'next/link'
import { useState } from 'react'
import { EyeOff, MoreVertical, Pencil, Play, Settings, Star, ThumbsUp, Users } from 'lucide-react'
import { openLoginModal } from '@/app/login/LoginModal'
import { EDITOR_PATH } from '@/lib/siteNavigation'
import { useSyncedToggle } from '@/lib/useSyncedToggle'
import { favoriteGame, likeGame, unfavoriteGame, unlikeGame } from '@/app/server-actions/game-likes'
import { useUser } from '@/components/context/useUser'
import { useGamePlayLaunch } from '@/components/site/useGamePlayLaunch'
import { DeeplinkFeedbackModal } from '@/components/site/DeeplinkFeedbackModal'
import { PlayerAvatar } from '@/components/site/PlayerAvatar'
import type { PlayerLook } from '@/lib/siteAvatar'
import { urlOfGameThumbnail } from '@/lib/s3StorageFiles'
import { getPlayerRangeText } from '@/lib/getPlayerRangeText'
import { HumanWrittenNote } from '@/components/site/HumanWrittenNote'
import { displayedDescription } from '@/lib/siteDescriptions'
import { GameSettingsDialog } from './GameSettingsDialog'
import { GameThumbnailSlideshow } from './GameThumbnailSlideshow'
import { Button } from '@/shadcn/components/ui/button'
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from '@/shadcn/components/ui/dropdown-menu'

type GameDetailPageProps = {
    game: {
        gameId: number
        name: string
        description: string | null
        aiThumbnailUuid: string | null
        screenshotUuid: string | null
        creatorUserId: number
        creatorUsername: string
        concurrentPlayers: number
        isLikedByViewer: boolean
        isFavoritedByViewer: boolean
        totalVisits: number
        suggestedMinPlayers: number
        maxPlayers: number
        isPublished: boolean
        hasWorld: boolean
    }
    servers: {
        requestId: string
        players: { userId: number, username: string, look: PlayerLook }[]
        maxPlayers: number
    }[]
    isOwner: boolean
}

export function GameDetailPage({
    game,
    servers,
    isOwner,
}: GameDetailPageProps) {
    const user = useUser()
    const [gameSettings, setGameSettings] = useState({
        name: game.name,
        description: game.description,
        suggestedMinPlayers: game.suggestedMinPlayers,
        maxPlayers: game.maxPlayers,
        isPublished: game.isPublished,
    })
    const [isSettingsDialogOpen, setIsSettingsDialogOpen] = useState(false)
    const { feedbackState, launchTarget, isLaunchPending, launchGame, closeLaunchFeedback } = useGamePlayLaunch()
    const like = useSyncedToggle({
        isOn: game.isLikedByViewer,
        saveIsOn: async (isLiked) => (isLiked ? await likeGame(game.gameId) : await unlikeGame(game.gameId)).isLikedByViewer,
        errorMessage: 'Could not update like.',
    })
    const favorite = useSyncedToggle({
        isOn: game.isFavoritedByViewer,
        saveIsOn: async (isFavorited) => (isFavorited ? await favoriteGame(game.gameId) : await unfavoriteGame(game.gameId)).isFavoritedByViewer,
        errorMessage: 'Could not update favorite.',
    })

    const toggleWhenLoggedIn = (toggle: () => void) => {
        if (!user) {
            openLoginModal({ next: `${window.location.pathname}${window.location.search}` })
            return
        }
        toggle()
    }

    const handleLaunch = async ({ requestId }: { requestId: string | null }) => {
        await launchGame({
            gameId: game.gameId,
            gameName: gameSettings.name,
            requestId,
            loginNextUrl: `${window.location.pathname}${window.location.search}`,
        })
    }

    const handleSettingsSaved = (savedGameSettings: {
        name: string
        description: string | null
        suggestedMinPlayers: number
        maxPlayers: number
        isPublished: boolean
    }) => {
        setGameSettings(savedGameSettings)
    }

    const isPublished = gameSettings.isPublished
    const description = displayedDescription(gameSettings.description)
    let playLabel = 'Play'
    if (!game.hasWorld) {
        playLabel = 'Publish from the editor to play'
    } else if (!isPublished) {
        playLabel = 'Change Visibility to play'
    } else if (launchTarget === 'play') {
        playLabel = 'Launching...'
    }

    // There is nothing to start until the creator has saved a world, so Play waits for one. Edit is
    // the button that gets them there, and is never held back.
    const shouldDisablePlay = isLaunchPending || !isPublished || !game.hasWorld
    const shouldShowOwnerMenu = isOwner

    return (
        <div>
            <DeeplinkFeedbackModal
                feedbackState={feedbackState}
                onClose={closeLaunchFeedback}
            />
            {isOwner && (
                <GameSettingsDialog
                    game={{
                        gameId: game.gameId,
                        name: gameSettings.name,
                        description: gameSettings.description,
                        suggestedMinPlayers: gameSettings.suggestedMinPlayers,
                        maxPlayers: gameSettings.maxPlayers,
                        isPublished,
                    }}
                    isOpen={isSettingsDialogOpen}
                    onOpenChange={setIsSettingsDialogOpen}
                    onSaved={handleSettingsSaved}
                />
            )}

            <main className="site-container site-page">
                {/* Image + details side by side. */}
                <div className="grid gap-x-10 gap-y-6 md:grid-cols-[minmax(0,1fr)_18rem] md:items-stretch">
                    <div className="w-full min-w-0">
                        <div className="game-thumbnail">
                            <GameThumbnailSlideshow
                                name={gameSettings.name}
                                thumbnailUrls={[game.aiThumbnailUuid, game.screenshotUuid].flatMap(thumbnailUuid => urlOfGameThumbnail(thumbnailUuid) ?? [])}
                                sizes="(min-width: 1328px) 920px, (min-width: 768px) calc(100vw - 408px), calc(100vw - 42px)"
                            />
                        </div>
                    </div>

                    {/* Buttons — aligned to image height only */}
                    {/* The column takes the image's height without adding to it, so a long description scrolls instead of pushing Play down. */}
                    <div className="flex min-w-0 w-full flex-col gap-5 md:h-0 md:min-h-full md:justify-between">
                        <div className="flex flex-col gap-2">
                            <div className="flex items-start gap-3">
                                {/* Title / creator */}
                                <div className="min-w-0 flex-1">
                                    <h1 className="site-page-title mb-3 min-w-0 break-words">{gameSettings.name}</h1>
                                    <div className="mb-2 text-sm text-gray-500">
                                        <p className="mb-1.5">
                                            By{' '}
                                            <Link href={`/profile/${game.creatorUserId}`} className="transition-colors hover:text-gray-950">
                                                {game.creatorUsername}
                                            </Link>
                                        </p>
                                    </div>
                                </div>
                                {shouldShowOwnerMenu && (
                                    <DropdownMenu modal={false}>
                                        <DropdownMenuTrigger asChild>
                                            <button
                                                type="button"
                                                aria-label="Game Settings"
                                                className="inline-flex size-9 shrink-0 items-center justify-center rounded-xl text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2"
                                            >
                                                <MoreVertical className="size-5" />
                                            </button>
                                        </DropdownMenuTrigger>
                                        <DropdownMenuContent align="end" className="w-44">
                                            <DropdownMenuItem onClick={() => setIsSettingsDialogOpen(true)}>
                                                <Settings className="size-4" />
                                                Settings
                                            </DropdownMenuItem>
                                        </DropdownMenuContent>
                                    </DropdownMenu>
                                )}
                            </div>
                            <div className="flex items-center gap-4 self-start">
                                <button
                                    type="button"
                                    onClick={() => toggleWhenLoggedIn(like.toggle)}
                                    aria-label={like.isOn ? 'Unlike game' : 'Like game'}
                                    aria-pressed={like.isOn}
                                    className="inline-flex items-center text-gray-500 transition-colors hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2"
                                >
                                    <ThumbsUp className={`size-6 ${like.isOn ? 'fill-gray-950 text-gray-950' : ''}`} />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => toggleWhenLoggedIn(favorite.toggle)}
                                    aria-label={favorite.isOn ? 'Unfavorite game' : 'Favorite game'}
                                    aria-pressed={favorite.isOn}
                                    className="inline-flex items-center text-gray-500 transition-colors hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2"
                                >
                                    <Star className={`size-6 ${favorite.isOn ? 'fill-yellow-400 text-yellow-400' : ''}`} />
                                </button>
                            </div>
                        </div>

                        <div className="flex min-h-0 flex-col gap-3">
                            {description && (
                                <div className="flex min-h-0 flex-col gap-1">
                                    {/* TODO: AI-PICKED-VALUE: On phones, where there is no image beside it, a long description scrolls past 240px. */}
                                    <p className="scroll-fade min-h-0 overflow-y-auto whitespace-pre-line break-words text-sm leading-6 text-gray-600 max-md:max-h-60">{description}</p>
                                    <HumanWrittenNote />
                                </div>
                            )}

                            {!game.hasWorld && (
                                <p className="flex items-start gap-1.5 text-xs text-gray-500">
                                    <Pencil className="mt-0.5 size-3.5 shrink-0" />
                                    <span>Edit this game to finish setting it up.</span>
                                </p>
                            )}

                            {!isPublished && game.hasWorld && (
                                <p className="flex items-start gap-1.5 text-xs text-gray-500">
                                    <EyeOff className="mt-0.5 size-3.5 shrink-0" />
                                    <span>This game is private. Change Visibility to play.</span>
                                </p>
                            )}

                            <Button
                                type="button"
                                aria-label={playLabel}
                                onClick={() => handleLaunch({ requestId: null })}
                                disabled={shouldDisablePlay}
                                size="lg"
                                className="w-full"
                            >
                                <Play className="size-5 fill-white" />
                            </Button>

                            {isOwner && (
                                <Button
                                    asChild
                                    variant="outline"
                                    size="lg"
                                    className="game-edit-button w-full"
                                >
                                    <Link href={EDITOR_PATH(game.gameId)}>
                                        <Pencil className="size-4" />
                                        Edit
                                    </Link>
                                </Button>
                            )}
                        </div>
                    </div>

                    <div className="md:col-start-1">
                        {isPublished && (
                            <div className="mt-10">
                                {/* Servers */}
                                <div className="mb-4">
                                    <h2 className="site-section-title">Servers</h2>
                                    <div className="mt-2 flex flex-wrap items-center gap-3">
                                        {/* <span className="flex items-center gap-1.5 text-xs text-gray-500">
                                            <DoorOpen className="size-3.5" />
                                            {game.totalVisits.toLocaleString()} {game.totalVisits === 1 ? 'visit' : 'visits'}
                                        </span> */}
                                        <span className="text-xs text-gray-500">{getPlayerRangeText(gameSettings)}</span>
                                        <span className="flex items-center gap-1.5 text-xs text-gray-500">
                                            <Users className="size-3.5" />
                                            {game.concurrentPlayers.toLocaleString()} playing
                                        </span>
                                    </div>
                                </div>
                                {servers.length === 0 ? (
                                    <p className="text-xs text-gray-500">No active servers right now.</p>
                                ) : (
                                    <div className="space-y-4">
                                        {servers.map((server) => (
                                            <div key={server.requestId} className="flex flex-col gap-4 rounded-2xl border border-gray-200 bg-gray-50 px-6 py-5">
                                                <div className="flex items-center justify-between gap-4">
                                                    <span className="text-sm text-gray-500">
                                                        {server.players.length} / {server.maxPlayers} players
                                                    </span>
                                                    <button
                                                        type="button"
                                                        onClick={() => handleLaunch({ requestId: server.requestId })}
                                                        disabled={isLaunchPending}
                                                        className="site-button"
                                                    >
                                                        {launchTarget === server.requestId ? 'Joining...' : 'Join'}
                                                    </button>
                                                </div>

                                                {server.players.length > 0 && (
                                                    <div className="flex flex-wrap gap-2">
                                                        {server.players.map((player) => (
                                                            <Link
                                                                key={`${server.requestId}:${player.userId}`}
                                                                href={`/profile/${player.userId}`}
                                                                className="flex items-center gap-2 rounded-full border border-gray-200 bg-white py-1 pl-1 pr-3 text-sm text-gray-600 transition-colors hover:text-gray-950"
                                                            >
                                                                <PlayerAvatar look={player.look} isProfileAvatar={false} className="size-7" />
                                                                {player.username}
                                                            </Link>
                                                        ))}
                                                    </div>
                                                )}
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </div>
                        )}
                    </div>
                </div>
            </main>
        </div>
    )
}
