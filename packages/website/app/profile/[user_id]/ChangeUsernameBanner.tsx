'use client'

import { FormEvent, useState } from 'react'
import { useRouter } from 'next/navigation'
import { TriangleAlert } from 'lucide-react'
import { changeUsername } from '@/app/server-actions/change-username'
import { PlayerAvatar } from '@/components/site/PlayerAvatar'
import { Button } from '@/shadcn/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shadcn/components/ui/dialog'
import type { PlayerLook } from '@/lib/siteAvatar'
import { usernameMaxLength } from '@/lib/siteUsernames'
import { useUsernameAvailability, type UsernameFieldState } from './usernameAvailabilityState'

// Sits under the name as it is typed. The name is written with its @ so it reads as the name it
// will be shown as, and a taken name is called out in the same red as a form error, because it
// stops the name being saved just as surely.
const UsernameAvailabilityNote = ({ availability, username }: { availability: UsernameFieldState, username: string }) => {
    if (availability.type === 'empty') return <p className="mt-1 min-h-4 text-xs text-red-500">Please enter your username.</p>
    if (availability.type === 'problem') return <p className="mt-1 min-h-4 text-xs text-red-500">{availability.problem}</p>
    if (availability.type === 'looking') return <p className="mt-1 min-h-4 text-xs text-gray-400">Checking...</p>
    if (availability.type === 'available') return <p className="mt-1 min-h-4 text-xs text-emerald-600">{username} is available.</p>
    return <p className="mt-1 min-h-4 text-xs text-red-500">{username} is taken.</p>
}

// The dialog unmounts its content when it closes, so the field starts empty every time it opens.
const ChangeUsernameForm = ({ onCancel, onChanged }: { onCancel: () => void, onChanged: (username: string) => void }) => {
    const [username, setUsername] = useState('')
    const availability = useUsernameAvailability(username)
    const [isSaving, setIsSaving] = useState(false)
    const [saveError, setSaveError] = useState<string | null>(null)

    const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault()
        setIsSaving(true)
        setSaveError(null)
        try {
            const result = await changeUsername(username)
            if (result.type === 'error') {
                setSaveError(result.error)
                setIsSaving(false)
                return
            }
            onChanged(username)
        } catch {
            setSaveError('Could not change your username.')
            setIsSaving(false)
        }
    }

    return (
        <form onSubmit={onSubmit}>
            <DialogHeader className="px-6 pt-5 text-left">
                <DialogTitle>Set Username</DialogTitle>
                <DialogDescription>Pick wisely. This can only be done once!</DialogDescription>
            </DialogHeader>

            <div className="px-6 py-5">
                <input
                    name="username"
                    type="text"
                    required
                    maxLength={usernameMaxLength}
                    title=""
                    aria-label="Username"
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                    autoComplete="off"
                    disabled={isSaving}
                    className="w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"
                />
                <UsernameAvailabilityNote availability={availability} username={username} />
            </div>

            {saveError && (
                <p className="px-6 pb-4 text-sm text-red-600" role="alert">{saveError}</p>
            )}

            <DialogFooter className="px-6 pb-5 pt-0">
                <Button type="button" variant="secondary" onClick={onCancel} disabled={isSaving}>Cancel</Button>
                <Button type="submit" disabled={isSaving || availability.type !== 'available'} className="disabled:opacity-100">
                    {isSaving ? 'Saving...' : 'Save'}
                </Button>
            </DialogFooter>
        </form>
    )
}

// TODO: AI-PICKED-VALUE: The welcome's avatar is a 192px circle, bigger than the profile's 120px one, in a dialog 448px wide.
const WelcomeContent = ({ username, look, onContinue }: { username: string, look: PlayerLook, onContinue: () => void }) => (
    <div className="flex flex-col items-center px-6 py-8 text-center">
        <DialogTitle className="text-xl">Welcome, {username}!</DialogTitle>
        <DialogDescription className="mt-1">{"It's good to have you here."}</DialogDescription>
        <PlayerAvatar look={look} isProfileAvatar={true} className="mt-6 size-48" />
        <p className="mt-3 text-lg font-semibold text-gray-950">{username}</p>
        <Button type="button" onClick={onContinue} className="mt-6 w-full">Continue</Button>
    </div>
)

// TODO: AI-PICKED-VALUE: The banner takes the free credits banner's #ffd84a fill, #3d2400 text and #e05a00 icon, darkening to #f5cc2e on hover.
// Shown on your own profile while you still have the Guest name every account starts with. The whole banner opens the picker,
// and a saved name swaps the banner for a welcome. Closing the welcome refreshes the page, which then shows the new name.
export const ChangeUsernameBanner = ({ look }: { look: PlayerLook }) => {
    const router = useRouter()
    const [isPickerOpen, setIsPickerOpen] = useState(false)
    const [pickedUsername, setPickedUsername] = useState<string | null>(null)
    const [isWelcomeOpen, setIsWelcomeOpen] = useState(false)

    const closeWelcome = () => {
        setIsWelcomeOpen(false)
        router.refresh()
    }

    return (
        <>
            {pickedUsername === null && <button
                type="button"
                onClick={() => setIsPickerOpen(true)}
                className="group flex w-fit flex-col gap-4 rounded-xl bg-[#ffd84a] px-5 py-4 text-left text-[#3d2400] transition-colors hover:bg-[#f5cc2e] sm:flex-row sm:items-center sm:gap-8"
            >
                <div className="flex items-center gap-4">
                    <TriangleAlert className="size-7 shrink-0 text-[#e05a00]" />
                    <div>
                        <p className="font-semibold text-[#1a1205]">Welcome to Kube.new!</p>
                        <p className="text-sm">Level up! Set your username.</p>
                    </div>
                </div>
                <span className="shrink-0 rounded-xl bg-black px-4 py-2.5 text-center text-sm font-medium text-white transition-colors group-hover:bg-black/80">
                    Set Username
                </span>
            </button>}
            <Dialog open={isPickerOpen} onOpenChange={setIsPickerOpen}>
                <DialogContent className="gap-0 border-0 p-0 sm:max-w-sm" showCloseButton={false}>
                    <ChangeUsernameForm
                        onCancel={() => setIsPickerOpen(false)}
                        onChanged={(username) => {
                            setIsPickerOpen(false)
                            setPickedUsername(username)
                            setIsWelcomeOpen(true)
                        }}
                    />
                </DialogContent>
            </Dialog>
            <Dialog open={isWelcomeOpen} onOpenChange={(isOpen) => { if (!isOpen) closeWelcome() }}>
                <DialogContent className="gap-0 border-0 p-0 sm:max-w-md" showCloseButton={false}>
                    {pickedUsername !== null && <WelcomeContent username={pickedUsername} look={look} onContinue={closeWelcome} />}
                </DialogContent>
            </Dialog>
        </>
    )
}
