'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Check, MoreVertical, Settings, Trash2 } from 'lucide-react'
import { openLoginModal } from '@/app/login/LoginModal'
import { buyCatalogItem, updateCatalogItem, type CatalogItemSettings } from '@/app/server-actions/catalog'
import { CatalogItemSettingsDialog } from '@/components/site/CatalogItemSettingsDialog'
import { DeleteCatalogItemDialog } from './DeleteCatalogItemDialog'
import { Button } from '@/shadcn/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/shadcn/components/ui/alert-dialog'

export const CatalogItemSettingsMenu = ({ item }: { item: { catalogItemId: number } & CatalogItemSettings }) => {
    const router = useRouter()
    const [isSettingsOpen, setIsSettingsOpen] = useState(false)
    const [isDeleteOpen, setIsDeleteOpen] = useState(false)

    const saveSettings = async (nextSettings: CatalogItemSettings) => {
        const result = await updateCatalogItem({ catalogItemId: item.catalogItemId, ...nextSettings })
        if ('settings' in result) {
            setIsSettingsOpen(false)
            router.refresh()
        }
        return result
    }

    return (
        <>
            <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                    <button type="button" aria-label="Item Settings" className="inline-flex size-9 shrink-0 items-center justify-center rounded-xl text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2">
                        <MoreVertical className="size-5" />
                    </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-44">
                    <DropdownMenuItem onClick={() => setIsSettingsOpen(true)}>
                        <Settings className="size-4" />
                        Settings
                    </DropdownMenuItem>
                    <DropdownMenuItem variant="destructive" onClick={() => setIsDeleteOpen(true)}>
                        <Trash2 className="size-4" />
                        Delete
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
            <CatalogItemSettingsDialog
                title="Item Settings"
                description="People who already bought this item keep it, whatever you change here."
                submitLabel="Save"
                initialSettings={item}
                isOpen={isSettingsOpen}
                onOpenChange={setIsSettingsOpen}
                onSubmit={saveSettings}
            />
            <DeleteCatalogItemDialog catalogItemId={item.catalogItemId} itemName={item.name} isOpen={isDeleteOpen} onOpenChange={setIsDeleteOpen} />
        </>
    )
}

export const CatalogItemActions = ({ item, isOwned, isSignedIn }: {
    item: { catalogItemId: number, name: string, isListed: boolean }
    isOwned: boolean
    isSignedIn: boolean
}) => {
    const router = useRouter()
    const [isOwnedNow, setIsOwnedNow] = useState(isOwned)
    const [isBuying, setIsBuying] = useState(false)
    const [buyError, setBuyError] = useState<string | null>(null)
    const [isConfirmOpen, setIsConfirmOpen] = useState(false)
    const [isBuyErrorOpen, setIsBuyErrorOpen] = useState(false)

    const askToBuy = () => {
        if (!isSignedIn) {
            openLoginModal({ next: `${window.location.pathname}${window.location.search}` })
            return
        }
        setIsConfirmOpen(true)
    }

    const buy = async () => {
        setIsConfirmOpen(false)
        setIsBuying(true)
        setBuyError(null)
        try {
            const result = await buyCatalogItem(item.catalogItemId)
            if ('error' in result) {
                setBuyError(result.error)
                setIsBuyErrorOpen(true)
                // A refusal can mean the item changed since the page was drawn, like being unlisted, so show it as it is now.
                router.refresh()
                return
            }
            setIsOwnedNow(true)
        } catch {
            setBuyError('Could not get this item. Try again.')
            setIsBuyErrorOpen(true)
        } finally {
            setIsBuying(false)
        }
    }

    return (
        <div className="mt-auto flex flex-col gap-3">
            <span className="text-2xl font-semibold">Free</span>
            {isOwnedNow ? (
                <Button type="button" size="lg" variant="outline" disabled className="w-full">
                    <Check className="size-4" />
                    Owned
                </Button>
            ) : (
                <Button type="button" size="lg" className="w-full" disabled={isBuying || !item.isListed} onClick={askToBuy}>
                    {!item.isListed ? 'Unlisted' : isBuying ? 'Getting…' : 'Get'}
                </Button>
            )}
            <AlertDialog open={isBuyErrorOpen} onOpenChange={setIsBuyErrorOpen}>
                <AlertDialogContent className="sm:max-w-md">
                    <AlertDialogHeader>
                        <AlertDialogTitle>Couldn&apos;t get {item.name}</AlertDialogTitle>
                        <AlertDialogDescription>{buyError}</AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogAction>OK</AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
            <AlertDialog open={isConfirmOpen} onOpenChange={setIsConfirmOpen}>
                <AlertDialogContent className="sm:max-w-md">
                    <AlertDialogHeader>
                        <AlertDialogTitle>Get</AlertDialogTitle>
                        <AlertDialogDescription>Get {item.name} for free?</AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <Button type="button" onClick={() => void buy()}>Get</Button>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    )
}
