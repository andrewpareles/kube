'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Trash2 } from 'lucide-react'
import { deleteAsset } from '@/app/server-actions/assets'
import { ASSETS_PATH } from '@/lib/siteNavigation'
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

type DeleteAssetDialogProps = {
    assetId: number
    assetName: string
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
}

export const DeleteAssetDialog = ({
    assetId,
    assetName,
    isOpen,
    onOpenChange,
}: DeleteAssetDialogProps) => {
    const router = useRouter()
    const [isDeleting, setIsDeleting] = useState(false)
    const [deleteError, setDeleteError] = useState<string | null>(null)

    const handleDelete = async () => {
        setIsDeleting(true)
        setDeleteError(null)
        try {
            const result = await deleteAsset(assetId)
            if ('error' in result) {
                setDeleteError(result.error)
                return
            }
            onOpenChange(false)
            router.push(ASSETS_PATH)
        } catch {
            setDeleteError('Could not delete the asset. Try again.')
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
                        <AlertDialogTitle>Delete {assetName}?</AlertDialogTitle>
                    </div>
                    <AlertDialogDescription>
                        This removes the asset from Assets. Games that already use it keep it. This cannot be undone.
                    </AlertDialogDescription>
                </AlertDialogHeader>

                {deleteError && (
                    <p className="text-sm text-red-600" role="alert">{deleteError}</p>
                )}

                <AlertDialogFooter>
                    <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
                    <Button type="button" variant="destructive" onClick={() => void handleDelete()} disabled={isDeleting}>
                        {isDeleting ? 'Deleting...' : 'Delete'}
                    </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    )
}
