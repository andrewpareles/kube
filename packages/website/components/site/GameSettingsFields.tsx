'use client'

import { Globe2, Lock } from 'lucide-react'
import { Input } from '@/shadcn/components/ui/input'
import { Switch } from '@/shadcn/components/ui/switch'
import { Textarea } from '@/shadcn/components/ui/textarea'
import { MAX_PLAYERS_LIMIT, type GameSettingsFormValues } from '@/lib/gameSettingsForm'

type GameSettingsFieldsProps = {
    formValues: GameSettingsFormValues
    isDisabled: boolean
    onFormValuesChange: (formValues: GameSettingsFormValues) => void
}

// The title, description, suggested min and max players and visibility rows, drawn the same way whether the game is
// being made for the first time or is already there and being edited.
export const GameSettingsFields = ({
    formValues,
    isDisabled,
    onFormValuesChange,
}: GameSettingsFieldsProps) => {
    const { name, description, suggestedMinPlayersInput, maxPlayersInput, isPublished } = formValues

    return (
        <>
            <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                <div>
                    <label htmlFor="game-settings-name" className="text-sm font-medium text-gray-950">Title</label>
                </div>
                <Input
                    id="game-settings-name"
                    value={name}
                    onChange={(event) => onFormValuesChange({ ...formValues, name: event.target.value })}
                    disabled={isDisabled}
                    className="bg-white"
                />
            </div>

            <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                <div>
                    <label htmlFor="game-settings-description" className="text-sm font-medium text-gray-950">Description</label>
                    <p className="mt-1 text-xs text-gray-500">Max 250 characters.</p>
                </div>
                <Textarea
                    id="game-settings-description"
                    value={description}
                    onChange={(event) => onFormValuesChange({ ...formValues, description: event.target.value })}
                    maxLength={250}
                    disabled={isDisabled}
                    className="min-h-24 resize-none bg-white"
                />
            </div>

            <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                <div>
                    <label htmlFor="game-settings-suggested-min-players" className="text-sm font-medium text-gray-950">Suggested Min Players</label>
                    <p className="mt-1 text-xs text-gray-500">Not enforced.</p>
                </div>
                <Input
                    id="game-settings-suggested-min-players"
                    type="number"
                    min={1}
                    max={MAX_PLAYERS_LIMIT}
                    step={1}
                    title=""
                    value={suggestedMinPlayersInput}
                    onChange={(event) => onFormValuesChange({ ...formValues, suggestedMinPlayersInput: event.target.value })}
                    disabled={isDisabled}
                    className="max-w-28 bg-white"
                />
            </div>

            <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                <div>
                    <label htmlFor="game-settings-max-players" className="text-sm font-medium text-gray-950">Max Players</label>
                </div>
                <Input
                    id="game-settings-max-players"
                    type="number"
                    min={1}
                    max={MAX_PLAYERS_LIMIT}
                    step={1}
                    title=""
                    value={maxPlayersInput}
                    onChange={(event) => onFormValuesChange({ ...formValues, maxPlayersInput: event.target.value })}
                    disabled={isDisabled}
                    className="max-w-28 bg-white"
                />
            </div>

            <div className="grid gap-3 sm:grid-cols-[9rem_minmax(0,1fr)]">
                <div>
                    <p className="text-sm font-medium text-gray-950">Visibility</p>
                </div>
                <div className="flex items-center justify-between rounded-xl bg-gray-50 px-4 py-3">
                    <div className="flex min-w-0 items-center gap-3">
                        {isPublished ? <Globe2 className="size-4 text-gray-700" /> : <Lock className="size-4 text-gray-500" />}
                        <div>
                            <p className="text-sm font-medium text-gray-950">{isPublished ? 'Published' : 'Private'}</p>
                            <p className="text-xs text-gray-500">{isPublished ? 'Visible on our Games page.' : 'Only you can view.'}</p>
                        </div>
                    </div>
                    <Switch
                        checked={isPublished}
                        onCheckedChange={(checked) => onFormValuesChange({ ...formValues, isPublished: checked })}
                        disabled={isDisabled}
                        aria-label="Publish game"
                    />
                </div>
            </div>
        </>
    )
}
