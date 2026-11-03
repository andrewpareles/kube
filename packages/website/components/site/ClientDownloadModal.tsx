'use client'

import { Download } from 'lucide-react'
import { ModalFrame } from '@/components/site/SiteModalFrame'
import { CLIENT_DOWNLOAD_PAGE_PATH } from '@/lib/clientDownloadUrls'

// Desktop play offers the app download when the browser cannot open it.
type ClientDownloadModalProps = {
    isOpen: boolean
    onClose: () => void
    autoDetectedDownloadUrl: string | null
}

export const ClientDownloadModal = ({
    isOpen,
    onClose,
    autoDetectedDownloadUrl,
}: ClientDownloadModalProps) => {
    const downloadHref = autoDetectedDownloadUrl ?? CLIENT_DOWNLOAD_PAGE_PATH

    return (
        <ModalFrame isOpen={isOpen} onClose={onClose}>
            <div className="space-y-6">
                <div className="pr-10">
                    <h2 className="mb-2 site-section-title">Download</h2>
                    <p className="text-gray-500">Download the app to continue.</p>
                </div>

                <div className='pb-4'>

                    <a
                        href={downloadHref}
                        className="flex w-full items-center justify-center gap-2 rounded-xl bg-primary px-4 py-2.5 font-medium text-white transition-colors hover:bg-primary/90"
                    >
                        <Download className="size-4 shrink-0" />
                        Download kube.new
                    </a>
                </div>
            </div>
        </ModalFrame>
    )
}
