'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Trash2 } from 'lucide-react'
import { deleteGame } from '@/app/server-actions/game-settings'
import { Button } from '@/shadcn/components/ui/button'
import {
    AlertDialog,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from '@/shadcn/components/ui/alert-dialog'

type DeleteGameDialogProps = {
    gameId: number
    gameName: string
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
}

export const DeleteGameDialog = ({
    gameId,
    gameName,
    isOpen,
    onOpenChange,
}: DeleteGameDialogProps) => {
    const router = useRouter()
    const [isDeleting, setIsDeleting] = useState(false)
    const [deleteError, setDeleteError] = useState<string | null>(null)

    const handleDelete = async () => {
        setIsDeleting(true)
        setDeleteError(null)
        try {
            const deleted = await deleteGame(gameId)
            if ('error' in deleted) {
                setDeleteError(deleted.error)
                return
            }
            const { profilePath } = deleted
            onOpenChange(false)
            router.push(profilePath)
        } catch (error) {
            setDeleteError(error instanceof Error ? error.message : 'Could not delete game.')
        } finally {
            setIsDeleting(false)
        }
    }

    const handleOverlayClick = () => {
        if (isDeleting) {
            return
        }

        onOpenChange(false)
    }

    return (
        <AlertDialog open={isOpen} onOpenChange={onOpenChange}>
            <AlertDialogContent className="sm:max-w-md" onOverlayClick={handleOverlayClick}>
                <AlertDialogHeader>
                    <div className="flex items-center gap-3">
                        <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-red-50 text-red-600">
                            <Trash2 className="size-5" />
                        </div>
                        <AlertDialogTitle>Delete {gameName}?</AlertDialogTitle>
                    </div>
                    <AlertDialogDescription>
                        This removes the game, its likes and visits, and its edit history. This cannot be undone.
                    </AlertDialogDescription>
                </AlertDialogHeader>

                {deleteError && (
                    <p className="text-sm text-red-600" role="alert">{deleteError}</p>
                )}

                <AlertDialogFooter>
                    <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
                    <Button type="button" variant="destructive" onClick={handleDelete} disabled={isDeleting}>
                        {isDeleting ? 'Deleting...' : 'Delete'}
                    </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    )
}
