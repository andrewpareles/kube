'use client'

import { useEffect, useState } from 'react'
import { ChevronDown, SlidersHorizontal, Trash2 } from 'lucide-react'
import { updateAssetSettings } from '@/app/server-actions/assets'
import { Button } from '@/shadcn/components/ui/button'
import {
    Dialog,
    DialogContent,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '@/shadcn/components/ui/dialog'
import { Input } from '@/shadcn/components/ui/input'
import { DeleteAssetDialog } from './DeleteAssetDialog'

type AssetSettingsDialogProps = {
    assetId: number
    assetName: string
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
    onSaved: () => void
}

export const AssetSettingsDialog = ({
    assetId,
    assetName,
    isOpen,
    onOpenChange,
    onSaved,
}: AssetSettingsDialogProps) => {
    const [name, setName] = useState(assetName)
    const [isDangerZoneOpen, setIsDangerZoneOpen] = useState(false)
    const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
    const [isSaving, setIsSaving] = useState(false)
    const [settingsError, setSettingsError] = useState<string | null>(null)

    useEffect(() => {
        if (!isOpen) {
            return
        }

        setName(assetName)
        setIsDangerZoneOpen(false)
        setIsDeleteDialogOpen(false)
        setSettingsError(null)
    }, [assetName, isOpen])

    const handleSave = async () => {
        setIsSaving(true)
        setSettingsError(null)
        try {
            const result = await updateAssetSettings({ assetId, name })
            if ('error' in result) {
                setSettingsError(result.error)
                return
            }
            onSaved()
            onOpenChange(false)
        } catch {
            setSettingsError('Could not save settings.')
        } finally {
            setIsSaving(false)
        }
    }

    return (
        <>
            <Dialog open={isOpen} onOpenChange={onOpenChange}>
                <DialogContent
                    className="site-settings-dialog max-h-[calc(100dvh-2rem)] gap-0 overflow-y-auto overscroll-contain border-0 p-0 sm:max-w-xl"
                    showCloseButton={false}
                    onOpenAutoFocus={(event) => event.preventDefault()}
                    onInteractOutside={(event) => event.preventDefault()}
                >
                    <DialogHeader className="px-6 pt-5 text-left">
                        <div className="flex items-center gap-3">
                            <div className="flex size-10 items-center justify-center rounded-xl bg-gray-100 text-gray-700">
                                <SlidersHorizontal className="size-5" />
                            </div>
                            <div>
                                <DialogTitle>Asset Settings</DialogTitle>
                            </div>
                        </div>
                    </DialogHeader>

                    <div className="space-y-5 px-6 py-5">
                        <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                            <div>
                                <label htmlFor="asset-settings-name" className="text-sm font-medium text-gray-950">Name</label>
                            </div>
                            <Input
                                id="asset-settings-name"
                                value={name}
                                onChange={(event) => setName(event.target.value)}
                                disabled={isSaving}
                                className="bg-white"
                            />
                        </div>


                        <div className="border-t border-gray-100 pt-5">
                            <button
                                type="button"
                                onClick={() => setIsDangerZoneOpen(!isDangerZoneOpen)}
                                className="flex w-full items-center justify-between text-left"
                            >
                                <span className="text-sm font-medium text-gray-950">Danger Zone</span>
                                <ChevronDown className={`size-4 text-gray-500 transition-transform ${isDangerZoneOpen ? 'rotate-180' : ''}`} />
                            </button>

                            {isDangerZoneOpen && (
                                <div className="mt-4 flex flex-col gap-4 rounded-xl bg-red-50 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
                                    <div>
                                        <p className="text-sm font-medium text-red-950">Delete Asset</p>
                                        <p className="mt-1 text-xs text-red-700">Irreversible. Games that already use it keep it.</p>
                                    </div>
                                    <Button
                                        type="button"
                                        variant="destructive"
                                        onClick={() => setIsDeleteDialogOpen(true)}
                                        disabled={isSaving}
                                    >
                                        <Trash2 className="size-4" />
                                        Delete
                                    </Button>
                                </div>
                            )}
                        </div>
                    </div>

                    {settingsError && (
                        <p className="px-6 text-sm text-red-600" role="alert">{settingsError}</p>
                    )}

                    <DialogFooter className="px-6 pb-5 pt-0">
                        <Button type="button" variant="secondary" onClick={() => onOpenChange(false)} disabled={isSaving}>Cancel</Button>
                        <Button type="button" onClick={() => void handleSave()} disabled={isSaving}>
                            {isSaving ? 'Saving...' : 'Save'}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
            <DeleteAssetDialog
                assetId={assetId}
                assetName={assetName}
                isOpen={isDeleteDialogOpen}
                onOpenChange={setIsDeleteDialogOpen}
            />
        </>
    )
}
