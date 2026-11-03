'use client'

import { useEffect, useState } from 'react'
import { ChevronDown, SlidersHorizontal, Trash2 } from 'lucide-react'
import { updateGameSettings, type SavedGameSettings } from '@/app/server-actions/game-settings'
import { Button } from '@/shadcn/components/ui/button'
import {
    Dialog,
    DialogContent,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '@/shadcn/components/ui/dialog'
import { GameSettingsFields } from '@/components/site/GameSettingsFields'
import { getGameSettingsFormError, type GameSettingsFormValues } from '@/lib/gameSettingsForm'
import { DeleteGameDialog } from './DeleteGameDialog'

export type EditableGameSettings = {
    gameId: number
    name: string
    description: string | null
    suggestedMinPlayers: number
    maxPlayers: number
    isPublished: boolean
}

type GameSettingsDialogProps = {
    game: EditableGameSettings
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
    onSaved: (savedGameSettings: SavedGameSettings) => void
    inviteToken?: string | null
    canDeleteGame?: boolean
}

export const GameSettingsDialog = ({
    game,
    isOpen,
    onOpenChange,
    onSaved,
    inviteToken = null,
    canDeleteGame = true,
}: GameSettingsDialogProps) => {
    const [formValues, setFormValues] = useState<GameSettingsFormValues>(() => ({
        name: game.name,
        description: game.description ?? '',
        suggestedMinPlayersInput: game.suggestedMinPlayers.toString(),
        maxPlayersInput: game.maxPlayers.toString(),
        isPublished: game.isPublished,
    }))
    const [isDangerZoneOpen, setIsDangerZoneOpen] = useState(false)
    const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
    const [isSaving, setIsSaving] = useState(false)
    const [settingsError, setSettingsError] = useState<string | null>(null)

    useEffect(() => {
        if (!isOpen) {
            return
        }

        setFormValues({
            name: game.name,
            description: game.description ?? '',
            suggestedMinPlayersInput: game.suggestedMinPlayers.toString(),
            maxPlayersInput: game.maxPlayers.toString(),
            isPublished: game.isPublished,
        })
        setIsDangerZoneOpen(false)
        setIsDeleteDialogOpen(false)
        setSettingsError(null)
    }, [game.description, game.isPublished, game.maxPlayers, game.suggestedMinPlayers, game.name, isOpen])

    const handleSave = async () => {
        const formError = getGameSettingsFormError(formValues)
        if (formError !== null) {
            setSettingsError(formError)
            return
        }

        setIsSaving(true)
        setSettingsError(null)
        try {
            const savedGameSettings = await updateGameSettings({
                gameId: game.gameId,
                name: formValues.name,
                description: formValues.description,
                suggestedMinPlayers: Number(formValues.suggestedMinPlayersInput),
                maxPlayers: Number(formValues.maxPlayersInput),
                isPublished: formValues.isPublished,
            }, inviteToken)
            if ('error' in savedGameSettings) {
                setSettingsError(savedGameSettings.error)
                return
            }
            onSaved(savedGameSettings)
            onOpenChange(false)
        } catch (error) {
            setSettingsError(error instanceof Error ? error.message : 'Could not save settings.')
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
                                <DialogTitle>Game Settings</DialogTitle>
                            </div>
                        </div>
                    </DialogHeader>

                    <div className="space-y-5 px-6 py-5">
                        <GameSettingsFields
                            formValues={formValues}
                            isDisabled={isSaving}
                            onFormValuesChange={setFormValues}
                        />

                        {canDeleteGame && <div className="border-t border-gray-100 pt-5">
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
                                        <p className="text-sm font-medium text-red-950">Delete Game</p>
                                        <p className="mt-1 text-xs text-red-700">Irreversible. Consider changing Visibility instead.</p>
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
                        </div>}
                    </div>

                    {settingsError && (
                        <p className="px-6 text-sm text-red-600" role="alert">{settingsError}</p>
                    )}

                    <DialogFooter className="px-6 pb-5 pt-0">
                        <Button type="button" variant="secondary" onClick={() => onOpenChange(false)} disabled={isSaving}>Cancel</Button>
                        <Button type="button" onClick={handleSave} disabled={isSaving}>
                            {isSaving ? 'Saving...' : 'Save'}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
            <DeleteGameDialog
                gameId={game.gameId}
                gameName={game.name}
                isOpen={isDeleteDialogOpen}
                onOpenChange={setIsDeleteDialogOpen}
            />
        </>
    )
}
