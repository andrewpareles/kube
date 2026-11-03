'use client'

import { ClientDownloadModal } from '@/components/site/ClientDownloadModal'
import { ModalFrame } from '@/components/site/SiteModalFrame'
import { type DeeplinkFeedbackState } from '@/components/site/useDeeplinkFeedback'

type DeeplinkFeedbackModalProps = {
    feedbackState: DeeplinkFeedbackState
    onClose: () => void
}

export const DeeplinkFeedbackModal = ({
    feedbackState,
    onClose,
}: DeeplinkFeedbackModalProps) => {
    if (feedbackState.status === 'closed') {
        return null
    }

    if (feedbackState.status === 'offerDownload') {
        return (
            <ClientDownloadModal
                isOpen={true}
                onClose={onClose}
                autoDetectedDownloadUrl={feedbackState.autoDetectedDownloadUrl}
            />
        )
    }

    if (feedbackState.status === 'launchError') {
        return (
            <ModalFrame isOpen={true} onClose={onClose}>
                <div className="pr-10">
                    <h2 className="mb-2 site-section-title">Something Went Wrong</h2>
                    <p className="text-gray-500">Please try again.</p>
                </div>
            </ModalFrame>
        )
    }

    const description = feedbackState.status === 'waitingForServer'
        ? 'Waiting for a server...'
        : 'Opening kube.new...'

    return (
        <ModalFrame isOpen={true} onClose={onClose}>
            <div className="pr-10">
                <h2 className="mb-2 site-section-title">Loading...</h2>
                <p className="text-gray-500">{description}</p>
            </div>
        </ModalFrame>
    )
}
