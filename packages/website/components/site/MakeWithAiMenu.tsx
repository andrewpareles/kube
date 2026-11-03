'use client'

import { useState } from 'react'
import Link from 'next/link'
import { ArrowUpRight, MoreVertical, Plus } from 'lucide-react'
import { CREATE_PATH } from '@/lib/siteNavigation'
import { Button } from '@/shadcn/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shadcn/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'

export const MakeWithAiMenu = ({ menuLabel, title, description }: { menuLabel: string, title: string, description: string }) => {
    const [isOpen, setIsOpen] = useState(false)

    return (
        <>
            <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                    <button type="button" aria-label={menuLabel} className="inline-flex size-9 shrink-0 items-center justify-center rounded-xl text-gray-500 transition-colors hover:bg-gray-100 hover:text-gray-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2">
                        <MoreVertical className="size-5" />
                    </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-44">
                    <DropdownMenuItem onClick={() => setIsOpen(true)}>
                        <Plus className="size-4" />
                        Make
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
            <Dialog open={isOpen} onOpenChange={setIsOpen}>
                <DialogContent className="sm:max-w-md">
                    <DialogHeader>
                        <DialogTitle>{title}</DialogTitle>
                        <DialogDescription>{description}</DialogDescription>
                    </DialogHeader>
                    <DialogFooter>
                        <Button asChild>
                            <Link href={CREATE_PATH}>
                                Chat
                                <ArrowUpRight aria-hidden="true" />
                            </Link>
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
        </>
    )
}
