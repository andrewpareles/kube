'use client'

import Link from 'next/link'
import { useRouter } from 'next/navigation'
import { useState } from 'react'
import { ChevronDown, SlidersHorizontal } from 'lucide-react'
import { createGame } from '@/app/server-actions/game-settings'
import { Button } from '@/shadcn/components/ui/button'
import {
    Dialog,
    DialogContent,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from '@/shadcn/components/ui/dialog'
import { GameSettingsFields } from '@/components/site/GameSettingsFields'
import { getGameSettingsFormError, newGameFormValues, type GameSettingsFormValues } from '@/lib/gameSettingsForm'
import { CREATE_PATH, EDITOR_PATH } from '@/lib/siteNavigation'

// The dialog unmounts its content when it closes, so the form starts fresh every time it opens.
const NewGameForm = ({ onCancel }: { onCancel: () => void }) => {
    const router = useRouter()
    const [formValues, setFormValues] = useState<GameSettingsFormValues>(newGameFormValues)
    const [isConfiguringManually, setIsConfiguringManually] = useState(false)
    const [isCreating, setIsCreating] = useState(false)
    const [createError, setCreateError] = useState<string | null>(null)

    const handleCreate = async () => {
        const formError = getGameSettingsFormError(formValues)
        if (formError !== null) {
            setCreateError(formError)
            return
        }

        setIsCreating(true)
        setCreateError(null)
        try {
            const { gameId } = await createGame({
                name: formValues.name,
                description: formValues.description,
                suggestedMinPlayers: Number(formValues.suggestedMinPlayersInput),
                maxPlayers: Number(formValues.maxPlayersInput),
                isPublished: formValues.isPublished,
                initialMessage: null,
            })
            router.push(EDITOR_PATH(gameId))
        } catch (error) {
            setCreateError(error instanceof Error ? error.message : 'Could not create the game.')
            setIsCreating(false)
        }
    }

    return (
        <>
            <DialogHeader className="px-6 pt-5 text-left">
                <div className="flex items-center gap-3">
                    <div className="flex size-10 items-center justify-center rounded-xl bg-gray-100 text-gray-700">
                        <SlidersHorizontal className="size-5" />
                    </div>
                    <div>
                        <DialogTitle>New Game</DialogTitle>
                    </div>
                </div>
            </DialogHeader>

            {/* Filling this in by hand is the long way round, so the note says so before
                anyone starts typing, and links straight to the prompt box on the home page. */}
            <div className="mx-6 mt-5 rounded-xl bg-gray-50 px-4 py-3 text-sm text-gray-500">
                <Link href={CREATE_PATH} className="text-gray-950 underline underline-offset-4 hover:text-gray-600">
                    Send a new message
                </Link>{' '}
                to have AI set up the new game for you!
            </div>

            {/* Folded away behind a heading, the same disclosure the settings dialog uses
                for its own last section, so the fields are plainly optional. Creating
                without opening it makes the game on the defaults. */}
            <div className="px-6 py-5">
                <div className="border-t border-gray-100 pt-5">
                    <button
                        type="button"
                        onClick={() => setIsConfiguringManually(!isConfiguringManually)}
                        className="flex w-full items-center justify-between text-left"
                    >
                        <span className="text-sm font-medium text-gray-950">Create Manually Instead</span>
                        <ChevronDown className={`size-4 text-gray-500 transition-transform ${isConfiguringManually ? 'rotate-180' : ''}`} />
                    </button>

                    {isConfiguringManually && (
                        <div className="mt-5 space-y-5">
                            <GameSettingsFields
                                formValues={formValues}
                                isDisabled={isCreating}
                                onFormValuesChange={setFormValues}
                            />
                        </div>
                    )}
                </div>
            </div>

            {createError && (
                <p className="px-6 text-sm text-red-600" role="alert">{createError}</p>
            )}

            <DialogFooter className="px-6 pb-5 pt-0">
                <Button type="button" variant="secondary" onClick={onCancel} disabled={isCreating}>Cancel</Button>
                <Button type="button" onClick={handleCreate} disabled={isCreating || !formValues.name.trim()} className="disabled:bg-muted disabled:text-muted-foreground disabled:opacity-100">
                    {isCreating ? 'Creating...' : 'Create'}
                </Button>
            </DialogFooter>
        </>
    )
}

// The same settings form the game's own page uses, without the advanced part, with Create
// instead of Save.
export const NewGameDialog = ({ isOpen, onOpenChange }: { isOpen: boolean, onOpenChange: (isOpen: boolean) => void }) => (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
        <DialogContent
            className="max-h-[calc(100dvh-2rem)] gap-0 overflow-y-auto overscroll-contain border-0 p-0 sm:max-w-lg"
            showCloseButton={false}
            onOpenAutoFocus={(event) => event.preventDefault()}
        >
            <NewGameForm onCancel={() => onOpenChange(false)} />
        </DialogContent>
    </Dialog>
)
