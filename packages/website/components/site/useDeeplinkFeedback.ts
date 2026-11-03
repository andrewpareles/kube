'use client'

import { useState } from 'react'
import { type BrowserAfterDeepLink } from '@/app/deeplink/sendDeepLinks'
import { getAutoDetectedDownloadUrl } from '@/lib/clientDownloadUrls'

export type DeeplinkFeedbackState =
    | { status: 'closed' }
    | { status: 'waitingForServer' }
    | { status: 'openingApp' }
    | { status: 'offerDownload', autoDetectedDownloadUrl: string | null }
    | { status: 'launchError' }

// Play launches offer the app download if the browser stays on the website.
export const useDeeplinkFeedback = () => {
    const [feedbackState, setFeedbackState] = useState<DeeplinkFeedbackState>({
        status: 'closed',
    })

    const showWaitingForServer = () => {
        setFeedbackState({
            status: 'waitingForServer',
        })
    }

    const showOpeningApp = () => {
        setFeedbackState({
            status: 'openingApp',
        })
    }

    const handleDeepLinkResult = async (browserAfterDeepLink: BrowserAfterDeepLink) => {
        if (browserAfterDeepLink === 'went-away') {
            close()
            return
        }

        const autoDetectedDownloadUrl = await getAutoDetectedDownloadUrl()
        if (document.hidden || !document.hasFocus()) {
            close()
            return
        }
        setFeedbackState({
            status: 'offerDownload',
            autoDetectedDownloadUrl,
        })
    }

    const showLaunchError = () => {
        setFeedbackState({
            status: 'launchError',
        })
    }

    const close = () => {
        setFeedbackState({ status: 'closed' })
    }

    return {
        feedbackState,
        showWaitingForServer,
        showOpeningApp,
        handleDeepLinkResult,
        showLaunchError,
        close,
    }
}
