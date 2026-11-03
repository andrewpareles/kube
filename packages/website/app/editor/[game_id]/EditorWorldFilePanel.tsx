'use client'

import { useState } from 'react'
import { Download } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/shadcn/components/ui/button'
import { AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/shadcn/components/ui/alert-dialog'
import { EditorFileDropZone, EditorSidePanelHeader, mainDropZoneClass } from './EditorSidePanel'

const chatRunningReason = 'Uploading is off while the AI is making changes.'

export const EditorWorldFilePanel = ({ editor, onClose }: { editor: { isChatRunning: boolean, downloadWorld: () => void, uploadWorld: (world: string) => Promise<void> } | null, onClose: () => void }) => {
    const [isUploading, setIsUploading] = useState(false)
    const [world, setWorld] = useState<string | null>(null)
    const isChatRunning = editor?.isChatRunning ?? false
    const chooseWorldFile = async ([file]: File[]) => {
        if (!file) return
        if (!/\.kube$/i.test(file.name)) { toast.error('Choose a .kube world file.'); return }
        setWorld(await file.text())
    }
    const uploadWorld = async () => {
        if (!editor || world === null) return
        setIsUploading(true)
        try { await editor.uploadWorld(world) } finally { setIsUploading(false); setWorld(null) }
    }
    return <section aria-label="World file" className="flex h-full min-h-0 flex-col">
        <EditorSidePanelHeader title="World file" onClose={onClose} />
        <div className="grid gap-2 px-3 pb-3">
            <Button type="button" variant="outline" size="sm" disabled={!editor} onClick={() => editor?.downloadWorld()}><Download />Download World</Button>
            <EditorFileDropZone label="Drop a .kube file, or click to browse" accept=".kube" isMultiple={false} isDisabled={!editor || isChatRunning || isUploading} onFiles={files => { chooseWorldFile(files).catch(error => toast.error(String(error))) }} className={mainDropZoneClass} />
            {isChatRunning && <p className="text-[11px]/4 text-gray-500">{chatRunningReason}</p>}
        </div>
        <AlertDialog open={world !== null} onOpenChange={isOpen => { if (!isOpen && !isUploading) setWorld(null) }}>
            <AlertDialogContent className="sm:max-w-md">
                <AlertDialogHeader>
                    <AlertDialogTitle>Replace the world?</AlertDialogTitle>
                    <AlertDialogDescription>This will override the current world for everyone in this editor, and clear the undo history.</AlertDialogDescription>
                </AlertDialogHeader>
                {isChatRunning && <p className="text-sm text-muted-foreground">{chatRunningReason}</p>}
                <AlertDialogFooter>
                    <AlertDialogCancel disabled={isUploading}>Cancel</AlertDialogCancel>
                    <Button type="button" variant="destructive" disabled={isUploading || isChatRunning} onClick={() => void uploadWorld()}>{isUploading ? 'Uploading…' : 'Replace World'}</Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    </section>
}
