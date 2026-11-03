'use client'

import { useState } from 'react'
import { MoreVertical, Plus } from 'lucide-react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'
import { NewGameDialog } from './NewGameDialog'

// On your own profile, the menu beside your name makes a new game.
export const ProfileMenu = () => {
    const [isNewGameOpen, setIsNewGameOpen] = useState(false)

    return (
        <>
            <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                    <button
                        type="button"
                        aria-label="Profile options"
                        className="inline-flex size-9 shrink-0 items-center justify-center rounded-xl text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2"
                    >
                        <MoreVertical className="size-5" />
                    </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-44">
                    <DropdownMenuItem onSelect={() => setIsNewGameOpen(true)}>
                        <Plus className="size-4" />
                        New Game
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
            <NewGameDialog isOpen={isNewGameOpen} onOpenChange={setIsNewGameOpen} />
        </>
    )
}
