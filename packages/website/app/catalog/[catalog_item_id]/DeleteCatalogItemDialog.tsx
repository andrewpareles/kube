'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Trash2 } from 'lucide-react'
import { deleteCatalogItem } from '@/app/server-actions/catalog'
import { CATALOG_PATH } from '@/lib/siteNavigation'
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

export const DeleteCatalogItemDialog = ({ catalogItemId, itemName, isOpen, onOpenChange }: {
    catalogItemId: number
    itemName: string
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
}) => {
    const router = useRouter()
    const [isDeleting, setIsDeleting] = useState(false)
    const [deleteError, setDeleteError] = useState<string | null>(null)

    const handleDelete = async () => {
        setIsDeleting(true)
        setDeleteError(null)
        try {
            const result = await deleteCatalogItem(catalogItemId)
            if ('error' in result) {
                setDeleteError(result.error)
                return
            }
            onOpenChange(false)
            router.push(CATALOG_PATH)
        } catch {
            setDeleteError('Could not delete the item. Try again.')
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
                        <AlertDialogTitle>Delete {itemName}?</AlertDialogTitle>
                    </div>
                    <AlertDialogDescription>
                        Everyone who owns it loses it, with no refund, and games can no longer load it. This cannot be undone.
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
