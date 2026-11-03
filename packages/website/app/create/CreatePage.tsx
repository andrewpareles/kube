'use client'

import { useEffect, useRef, useState } from 'react'
import Link from 'next/link'
import { useRouter } from 'next/navigation'
import { ArrowRight, Pause, Play } from 'lucide-react'
import { usePlayerUserId, useUser } from '@/components/context/useUser'
import { GameRow } from '@/components/site/GameRow'
import { GameCardPlay, type GameCardPlayData } from '@/components/site/GameCardPlay'
import { GameThumbnailImage } from '@/components/site/GameThumbnailImage'
import { createGame } from '@/app/server-actions/game-settings'
import { openLoginModal } from '@/app/login/LoginModal'
import { NEW_GAME_DEFAULT_MAX_PLAYERS, NEW_GAME_DEFAULT_SUGGESTED_MIN_PLAYERS, NEW_GAME_DEFAULT_NAME } from '@/lib/gameSettingsForm'
import { useRefState } from '@/lib/useRefState'
import { CREATE_PATH, EDITOR_PATH, GAMES_PATH, PROFILE_PATH } from '@/lib/siteNavigation'
import { CreateGamePrompt } from './CreateGamePrompt'

// Keep one pending prompt in this tab while the user signs in.
const EDITOR_PROMPT_DRAFT_STORAGE_KEY = 'browserEditorPromptDraft'

// The home page is a taste of the games rather than the whole shelf, so it stops after three rows
// and hands the rest to the all-games page.
// The selected design shows two rows of four cards.
const CREATE_PAGE_GAME_COUNT = 8
const CREATE_PAGE_ALL_GAME_COUNT = 40
// The games whose thumbnails sit in a row below the fold, from left to right. Captured on 2026-10-11; the local PNG
// copies keep these thumbnails unchanged when the games are edited, unpublished, or removed.
const FOLD_GAMES = [
    { gameId: 282, name: 'Snowball Village Showdown', thumbnailUrl: '/site-fold/282.png', videoUrl: '/site-fold/282.mp4' },
    { gameId: 313, name: 'Zombies vs Humans: Quarantine Zone', thumbnailUrl: '/site-fold/313.png', videoUrl: '/site-fold/313.mp4' },
    { gameId: 294, name: 'Cheese Heist', thumbnailUrl: '/site-fold/294.png', videoUrl: '/site-fold/294.mp4' },
]

// The thumbnail stays on top until the video is playing, so the tile doesn't go blank while the video loads after a click, and
// fades back in when the video ends.
// TODO: AI-PICKED-VALUE: The thumbnail fades out and back in over 0.5 seconds.
const FoldGameVideo = ({ name, thumbnailUrl, videoUrl, onPlay }: { name: string, thumbnailUrl: string, videoUrl: string, onPlay: (video: HTMLVideoElement) => void }) => {
    const videoRef = useRef<HTMLVideoElement>(null)
    const [isVideoShowing, setIsVideoShowing] = useState(false)

    const toggleVideo = () => {
        const video = videoRef.current
        if (video === null) return
        if (video.paused) {
            video.currentTime = 0
            void video.play()
        } else {
            video.pause()
        }
    }

    return (
        <button type="button" aria-label={isVideoShowing ? `Pause ${name}` : `Play ${name}`} className="create-fold-game" onClick={toggleVideo}>
            <video ref={videoRef} src={videoUrl} preload="none" playsInline={true} aria-hidden={true} className="absolute inset-0 size-full object-cover" onPlay={(event) => onPlay(event.currentTarget)} onPlaying={() => setIsVideoShowing(true)} onPause={() => setIsVideoShowing(false)} />
            <GameThumbnailImage name={name} thumbnailUrl={thumbnailUrl} sizes="(max-width: 560px) 50vw, 372px" className={isVideoShowing ? 'transition-opacity duration-500 opacity-0' : 'transition-opacity duration-500'} isHighPriority={true} />
            <span aria-hidden={true} className="create-fold-game-control">{isVideoShowing ? <Pause className="size-3.5 fill-current" /> : <Play className="size-3.5 translate-x-px fill-current" />}</span>
        </button>
    )
}

type CreatePageProps = {
    myGames: GameCardPlayData[]
    allGames: GameCardPlayData[]
}

export function CreatePage({
    myGames,
    allGames,
}: CreatePageProps) {
    const visibleAllGames = allGames.slice(0, CREATE_PAGE_ALL_GAME_COUNT)
    const [input, setInput] = useState('')
    const [isSubmitting, setIsSubmitting] = useRefState(false)
    const [createError, setCreateError] = useState<string | null>(null)
    const router = useRouter()
    const user = useUser()
    const playerUserId = usePlayerUserId()
    const textareaRef = useRef<HTMLTextAreaElement>(null)
    const playingFoldVideoRef = useRef<HTMLVideoElement | null>(null)

    const pauseOtherFoldVideo = (video: HTMLVideoElement) => {
        if (playingFoldVideoRef.current !== null && playingFoldVideoRef.current !== video) playingFoldVideoRef.current.pause()
        playingFoldVideoRef.current = video
    }

    // The hero fills the screen under the header, so the prompt box is all there is on arrival.
    const foldClassName = 'create-hero'

    useEffect(() => {
        const prompt = sessionStorage.getItem(EDITOR_PROMPT_DRAFT_STORAGE_KEY)
        const frameId = window.requestAnimationFrame(() => {
            if (prompt !== null && prompt.trim()) {
                setInput((currentInput) => currentInput || prompt)
            }
        })

        return () => window.cancelAnimationFrame(frameId)
    }, [])

    // sending a prompt is the only thing that opens an editor here
    const startCreatingGame = async () => {
        if (!input.trim() || isSubmitting.current) return
        if (user === null) {
            sessionStorage.setItem(EDITOR_PROMPT_DRAFT_STORAGE_KEY, input)
            openLoginModal({ next: CREATE_PATH })
            return
        }
        setIsSubmitting(true)
        setCreateError(null)
        try {
            // Reuse website creation and clear the pending login prompt once it succeeds.
            const { gameId } = await createGame({
                name: NEW_GAME_DEFAULT_NAME,
                description: '',
                suggestedMinPlayers: NEW_GAME_DEFAULT_SUGGESTED_MIN_PLAYERS,
                maxPlayers: NEW_GAME_DEFAULT_MAX_PLAYERS,
                isPublished: false,
                initialMessage: input,
            })
            sessionStorage.removeItem(EDITOR_PROMPT_DRAFT_STORAGE_KEY)
            router.push(EDITOR_PATH(gameId))
        } catch (error) {
            setCreateError(error instanceof Error ? error.message : 'Could not create the game.')
            setIsSubmitting(false)
        }
    }

    return (
        <main>
            {/* ── Above the fold ── */}
            {/* pt-24 clears the fixed header with room to spare, including on short laptop screens
                where the fold content is taller than the space left for it. */}
            <section className={foldClassName}>
                {/* The mark's element is only as big as the K itself now, with none of the empty
                    square around it, so the gap below has to make up what that used to provide. */}
                <h1 className="create-headline text-balance">
                    What are we building today?
                </h1>

                {/* Chat box */}
                <CreateGamePrompt
                    input={input}
                    isSubmitting={isSubmitting.current}
                    textareaRef={textareaRef}
                    onInputChange={setInput}
                    onSubmit={startCreatingGame}
                />
                {createError && <p className="mt-3 text-sm text-red-600" role="alert">{createError}</p>}
            </section>

            {/* Game thumbnails, each fading in once its picture has loaded. */}
            <section className="create-fold-games-section">
                <div className="create-fold-games">
                    {FOLD_GAMES.map((game) => <FoldGameVideo key={game.gameId} name={game.name} thumbnailUrl={game.thumbnailUrl} videoUrl={game.videoUrl} onPlay={pauseOtherFoldVideo} />)}
                </div>
                <h2 className="create-fold-games-title text-balance">See what people are making.</h2>
            </section>

            {/* ── Play a game section ── */}
            <section className="site-container create-games">
                <div>
                    <div className="flex flex-col gap-14">
                        {myGames.length > 0 && playerUserId !== null && (
                            <div>
                                <GameRow
                                    title="My Games"
                                    titleHref={`${PROFILE_PATH}/${playerUserId}/games`}
                                    headingLevel="section"
                                >
                                    {myGames.slice(0, CREATE_PAGE_GAME_COUNT).map((game) => <GameCardPlay key={game.gameId} game={game} playerStat="live" />)}
                                </GameRow>
                                <div className="mt-8 flex justify-center">
                                    <Link
                                        href={`${PROFILE_PATH}/${playerUserId}/games`}
                                        className="inline-flex items-center gap-1.5 rounded-full bg-gray-600 px-5 py-2 text-sm font-medium text-white transition-colors hover:bg-gray-700"
                                    >
                                        My Games <ArrowRight className="size-3.5" aria-hidden="true" />
                                    </Link>
                                </div>
                            </div>
                        )}
                        <div>
                            <div className="mb-[23px] flex items-center justify-between gap-4">
                                <h2 className="site-section-title">
                                    <Link href={GAMES_PATH} className="game-section-link">All Games <ArrowRight className="game-section-arrow shrink-0" aria-hidden="true" /></Link>
                                </h2>
                            </div>
                            <GameRow
                                title="All Games"
                                titleHref={null}
                                headingLevel="none"
                            >
                                {visibleAllGames.map((game) => <GameCardPlay key={game.gameId} game={game} playerStat="live" />)}
                            </GameRow>
                            {/* Centered on the same width the cards occupy, so it reads as the end of
                                // the shelf rather than as something floating in the page. */}
                            <div className="mt-8 flex justify-center">
                                <Link
                                    href={GAMES_PATH}
                                    className="inline-flex items-center gap-1.5 rounded-full bg-primary px-5 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
                                >
                                    All Games <ArrowRight className="size-3.5" aria-hidden="true" />
                                </Link>
                            </div>
                        </div>
                    </div>
                </div>
            </section>
        </main>
    )
}
