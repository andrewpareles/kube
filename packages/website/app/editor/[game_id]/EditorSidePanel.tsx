'use client'

import { useState } from 'react'
import { Upload, X } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/shadcn/components/ui/button'

export const EditorSidePanelHeader = ({ title, onClose }: { title: string, onClose: () => void }) => <div className="flex shrink-0 items-center justify-between pr-1.5">
    <h2 className="shrink-0 px-3 py-2 text-xs font-semibold">{title}</h2>
    <Button type="button" variant="ghost" size="icon" aria-label={`Close ${title}`} className="size-6 rounded-sm text-gray-400 hover:text-gray-800 [&_svg]:size-3" onClick={onClose}><X /></Button>
</div>

// Chrome has no FileSystemFileEntry or FileSystemDirectoryEntry globals to check entries against, so entries are told apart by their flags.
const isFileEntry = (entry: FileSystemEntry): entry is FileSystemFileEntry => entry.isFile
const isDirectoryEntry = (entry: FileSystemEntry): entry is FileSystemDirectoryEntry => entry.isDirectory

// A dropped folder arrives as a directory entry, so it is read down to every file inside it.
const readFilesOfEntry = async (entry: FileSystemEntry): Promise<File[]> => {
    if (isFileEntry(entry)) return [await new Promise<File>((resolve, reject) => entry.file(resolve, reject))]
    if (!isDirectoryEntry(entry)) return []
    const reader = entry.createReader()
    const readBatch = () => new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject))
    const files: File[] = []
    // The reader hands back one batch of entries at a time, then an empty batch once the folder is done.
    for (let batch = await readBatch(); batch.length > 0; batch = await readBatch()) {
        for (const child of batch) files.push(...await readFilesOfEntry(child))
    }
    return files
}

// The browser empties a drop's item list once the event ends, so the entries are taken right away and read after.
const readDroppedFiles = (items: DataTransferItemList) => {
    const entries = [...items].flatMap(item => item.webkitGetAsEntry() ?? [])
    return Promise.all(entries.map(readFilesOfEntry)).then(files => files.flat())
}

// TODO: AI-PICKED-VALUE: A panel's main drop zone is 144px tall so it reads as the main target of the panel.
export const mainDropZoneClass = 'h-36 flex-col justify-center text-center text-balance [&_svg]:size-5'

export const EditorFileDropZone = ({ label, accept, isMultiple, isDisabled, onFiles, className }: { label: string, accept: string, isMultiple: boolean, isDisabled: boolean, onFiles: (files: File[]) => void, className: string | undefined }) => {
    const [isDragOver, setIsDragOver] = useState(false)
    return <label onDragOver={event => { event.preventDefault(); if (!isDisabled) setIsDragOver(true) }} onDragLeave={event => { if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) setIsDragOver(false) }} onDrop={event => { event.preventDefault(); setIsDragOver(false); if (!isDisabled) readDroppedFiles(event.dataTransfer.items).then(onFiles, error => toast.error(String(error))) }} className={`flex items-center gap-2 rounded-lg border border-dashed px-3 py-2.5 text-xs transition-colors has-[input:focus-visible]:ring-1 has-[input:focus-visible]:ring-blue-500 ${isDisabled ? 'cursor-default border-gray-200 text-gray-400' : isDragOver ? 'cursor-pointer border-blue-500 bg-blue-50 text-blue-700' : 'cursor-pointer border-gray-300 text-gray-500 hover:bg-gray-50'} ${className ?? ''}`}>
        <Upload aria-hidden="true" className="size-3.5 shrink-0" />
        <span className="min-w-0 wrap-anywhere">{label}</span>
        <input type="file" accept={accept} multiple={isMultiple} disabled={isDisabled} className="sr-only" onChange={event => { onFiles([...event.target.files ?? []]); event.target.value = '' }} />
    </label>
}
