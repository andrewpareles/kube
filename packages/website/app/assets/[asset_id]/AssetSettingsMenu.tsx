'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { MoreVertical, Settings } from 'lucide-react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'
import { AssetSettingsDialog } from './AssetSettingsDialog'

export const AssetSettingsMenu = ({ assetId, assetName }: { assetId: number, assetName: string }) => {
    const router = useRouter()
    const [isSettingsDialogOpen, setIsSettingsDialogOpen] = useState(false)

    return (
        <>
            <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                    <button
                        type="button"
                        aria-label="Asset Settings"
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
            <AssetSettingsDialog
                assetId={assetId}
                assetName={assetName}
                isOpen={isSettingsDialogOpen}
                onOpenChange={setIsSettingsDialogOpen}
                onSaved={() => router.refresh()}
            />
        </>
    )
}
