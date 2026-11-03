'use client'

import { useEffect, useState } from 'react'
import { Globe2, Lock } from 'lucide-react'
import type { CatalogItemSettings } from '@/app/server-actions/catalog'
import { Button } from '@/shadcn/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shadcn/components/ui/dialog'
import { Input } from '@/shadcn/components/ui/input'
import { Label } from '@/shadcn/components/ui/label'
import { Switch } from '@/shadcn/components/ui/switch'
import { Textarea } from '@/shadcn/components/ui/textarea'

export const CatalogItemSettingsDialog = ({ title, description, submitLabel, initialSettings, isOpen, onOpenChange, onSubmit }: {
    title: string
    description: string
    submitLabel: string
    initialSettings: CatalogItemSettings
    isOpen: boolean
    onOpenChange: (isOpen: boolean) => void
    onSubmit: (settings: CatalogItemSettings) => Promise<{ error: string } | object>
}) => {
    const [name, setName] = useState(initialSettings.name)
    const [itemDescription, setItemDescription] = useState(initialSettings.description ?? '')
    const [isListed, setIsListed] = useState(initialSettings.isListed)
    const [isPending, setIsPending] = useState(false)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => {
        if (!isOpen) return
        setName(initialSettings.name)
        setItemDescription(initialSettings.description ?? '')
        setIsListed(initialSettings.isListed)
        setError(null)
    }, [initialSettings, isOpen])

    const run = async (action: () => Promise<{ error: string } | object>) => {
        setIsPending(true)
        setError(null)
        try {
            const result = await action()
            if ('error' in result) setError(result.error)
        } catch {
            setError('Something went wrong. Try again.')
        } finally {
            setIsPending(false)
        }
    }

    const submit = () => {
        void run(() => onSubmit({ name, description: itemDescription, isListed }))
    }

    return (
        <Dialog open={isOpen} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-lg">
                <DialogHeader>
                    <DialogTitle>{title}</DialogTitle>
                    <DialogDescription>{description}</DialogDescription>
                </DialogHeader>
                <div className="flex flex-col gap-4">
                    <div className="flex flex-col gap-2">
                        <Label htmlFor="catalog-item-name">Name</Label>
                        <Input id="catalog-item-name" value={name} maxLength={100} onChange={event => setName(event.target.value)} />
                    </div>
                    <div className="flex flex-col gap-2">
                        <Label htmlFor="catalog-item-description">Description</Label>
                        <Textarea id="catalog-item-description" value={itemDescription} maxLength={1000} onChange={event => setItemDescription(event.target.value)} />
                    </div>
                    <div className="flex flex-col gap-2">
                        <Label htmlFor="catalog-item-public">Listing</Label>
                        <div className="flex items-center justify-between rounded-xl bg-gray-50 px-4 py-3">
                            <div className="flex min-w-0 items-center gap-3">
                                {isListed ? <Globe2 className="size-4 text-gray-700" /> : <Lock className="size-4 text-gray-500" />}
                                <div>
                                    <p className="text-sm font-medium text-gray-950">{isListed ? 'Listed' : 'Unlisted'}</p>
                                    <p className="text-xs text-gray-500">{isListed ? 'Shown on our Catalog page, where people can buy it.' : 'Not on the Catalog page and can\'t be bought. Anyone with its id can still use it.'}</p>
                                </div>
                            </div>
                            <Switch id="catalog-item-public" checked={isListed} onCheckedChange={setIsListed} disabled={isPending} aria-label="List item on the Catalog page" />
                        </div>
                    </div>
                    {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
                </div>
                <DialogFooter className="gap-2">
                    <Button type="button" variant="outline" disabled={isPending} onClick={() => onOpenChange(false)}>Cancel</Button>
                    <Button type="button" disabled={isPending} onClick={submit}>{submitLabel}</Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    )
}
