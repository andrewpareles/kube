'use client'

import { useState } from 'react'
import { getJoinGameInfoAnyGame, getJoinGameInfoSpecificGame } from '@/app/server-actions/edgegap-actions'
import { openLoginModal } from '@/app/login/LoginModal'
import { sendDeepLinkPlayMode } from '@/app/deeplink/sendDeepLinks'
import { useUser } from '@/components/context/useUser'
import { useDeeplinkFeedback } from './useDeeplinkFeedback'

export const useGamePlayLaunch = () => {
    const [launchTarget, setLaunchTarget] = useState<string | null>(null)
    const user = useUser()
    const deeplinkFeedback = useDeeplinkFeedback()

    const launchGame = async ({
        gameId,
        gameName,
        requestId,
        loginNextUrl,
    }: {
        gameId: number
        gameName: string
        requestId: string | null
        loginNextUrl: string
    }) => {
        // if not logged in, ask for auth
        if (!user) {
            openLoginModal({ next: loginNextUrl })
            return
        }

        // launch
        const launchTargetKey = requestId ?? 'play'
        setLaunchTarget(launchTargetKey)
        deeplinkFeedback.showWaitingForServer()

        try {
            const playLaunchInfo = requestId
                ? await getJoinGameInfoSpecificGame(gameId, requestId)
                : await getJoinGameInfoAnyGame(gameId)
            deeplinkFeedback.showOpeningApp()
            const browserAfterDeepLink = await sendDeepLinkPlayMode({
                requestId: playLaunchInfo.requestId,
                playAuth: playLaunchInfo.playAuth,
                thumbnailUuid: playLaunchInfo.thumbnailUuid,
                thumbnailGameName: gameName,
            })
            await deeplinkFeedback.handleDeepLinkResult(browserAfterDeepLink)
        } catch {
            deeplinkFeedback.showLaunchError()
        }
        setLaunchTarget(null)
    }

    return {
        feedbackState: deeplinkFeedback.feedbackState,
        launchTarget,
        isLaunchPending: launchTarget !== null,
        launchGame,
        closeLaunchFeedback: deeplinkFeedback.close,
    }
}
