'use client'

import { useState } from 'react'
import Link from 'next/link'
import { ArrowUpRight, Check, LoaderCircle, X } from 'lucide-react'
import { Button } from '@/shadcn/components/ui/button'
import { Checkbox } from '@/shadcn/components/ui/checkbox'
import { Input } from '@/shadcn/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shadcn/components/ui/tabs'
import { ASSETS_PATH } from '@/lib/siteNavigation'
import { EditorFileDropZone, EditorSidePanelHeader, mainDropZoneClass } from './EditorSidePanel'

export type EditorAssetKind = 'Mesh' | 'Texture' | 'Sound' | 'Skybox'
type MaterialTextureSlot = 'albedo' | 'normal' | 'roughness' | 'metallic'
export type MaterialTextureFileOfSlot = { [slot in MaterialTextureSlot]: File | null }
// An upload resolves to the id of the asset it became, or null when it was only added to the world.
export type EditorAssetUploader = { uploadAsset: (file: File, kind: EditorAssetKind, name: string, shouldPublish: boolean) => Promise<number | null>, uploadMaterial: (name: string, textureFileOfSlot: MaterialTextureFileOfSlot, shouldPublish: boolean) => Promise<number | null> }
type AssetUpload = { id: string, file: File, kind: EditorAssetKind, name: string, status: 'ready' | 'uploading' | 'uploaded' | 'failed', assetId: number | null, error: string | null }
type ImportedUpload = { id: string, name: string, status: 'uploading' | 'uploaded', assetId: number | null }

const IMAGE_FILE_TYPES = '.png,.jpg,.jpeg,.webp'
const ASSET_FILE_TYPES = `.glb,${IMAGE_FILE_TYPES},.ogg,.mp3,.wav,.flac`
const materialTextures: { slot: MaterialTextureSlot, label: string, isRequired: boolean }[] = [
    { slot: 'albedo', label: 'Color texture', isRequired: true },
    { slot: 'normal', label: 'Normal texture', isRequired: false },
    { slot: 'roughness', label: 'Roughness texture', isRequired: false },
    { slot: 'metallic', label: 'Metalness texture', isRequired: false },
]
const emptyMaterialTextureFileOfSlot: MaterialTextureFileOfSlot = { albedo: null, normal: null, roughness: null, metallic: null }
const getNameOfFile = (file: File) => file.name.replace(/\.[^.]*$/, '')
const getAssetKind = (fileName: string): EditorAssetKind | null => /\.glb$/i.test(fileName) ? 'Mesh' : /\.(png|jpe?g|webp)$/i.test(fileName) ? 'Texture' : /\.(ogg|mp3|wav|flac)$/i.test(fileName) ? 'Sound' : null

const UploadName = ({ name, assetId }: { name: string, assetId: number | null }) => <span className="flex min-w-0 flex-1 items-center gap-0.5">
    <span className="truncate text-xs">{name}</span>
    {assetId !== null && <Button asChild variant="ghost" size="icon" className="size-7 shrink-0 rounded p-1 text-gray-500 hover:bg-gray-100"><Link href={`${ASSETS_PATH}/${assetId}`} target="_blank" aria-label={`Open ${name} in the Assets`}><ArrowUpRight className="size-4" /></Link></Button>}
</span>

const UploadToAssetsCheckbox = ({ shouldPublish, setShouldPublish }: { shouldPublish: boolean, setShouldPublish: (shouldPublish: boolean) => void }) => <label className="flex items-center gap-2 text-xs text-gray-700">
    <Checkbox checked={shouldPublish} onCheckedChange={checked => setShouldPublish(checked === true)} className="size-3.5 [&_svg]:size-3" />
    Upload to Assets too (Recommended)
</label>

const ImportedUploadList = ({ label, uploads }: { label: string, uploads: ImportedUpload[] }) => uploads.length === 0 ? null : <ul aria-label={label} className="grid gap-1.5">
    {uploads.map(upload => <li key={upload.id} className="flex min-h-7 items-center gap-1.5">
        <UploadName name={upload.name} assetId={upload.assetId} />
        {upload.status === 'uploading' ? <LoaderCircle aria-label="Importing" className="size-3.5 shrink-0 animate-spin text-gray-400" /> : <Check aria-label="Imported" className="size-3.5 shrink-0 text-emerald-600" />}
    </li>)}
</ul>

// Models, images, and sounds share one list, and each file's kind comes from its extension. A folder adds every such
// file inside it. Import sends the files that are not imported yet one after another, so a failed one can be retried.
const AssetFilesUploadForm = ({ uploader, shouldPublish, setShouldPublish }: { uploader: EditorAssetUploader | null, shouldPublish: boolean, setShouldPublish: (shouldPublish: boolean) => void }) => {
    const [uploads, setUploads] = useState<AssetUpload[]>([])
    const [skippedFileCount, setSkippedFileCount] = useState(0)
    const [isUploading, setIsUploading] = useState(false)
    const addFiles = (files: File[]) => {
        // A dropped folder brings its hidden files, like .DS_Store, which nobody meant to upload.
        const visibleFiles = files.filter(file => !file.name.startsWith('.'))
        const added = visibleFiles.flatMap((file): AssetUpload[] => {
            const kind = getAssetKind(file.name)
            return kind === null ? [] : [{ id: crypto.randomUUID(), file, kind, name: getNameOfFile(file), status: 'ready', assetId: null, error: null }]
        })
        setSkippedFileCount(visibleFiles.length - added.length)
        setUploads(previous => [...previous, ...added])
    }
    const updateUpload = (id: string, update: Partial<Pick<AssetUpload, 'name' | 'status' | 'assetId' | 'error'>>) => setUploads(previous => previous.map(upload => upload.id === id ? { ...upload, ...update } : upload))
    const waitingUploads = uploads.filter(upload => upload.status === 'ready' || upload.status === 'failed')
    const uploadAll = async () => {
        if (!uploader) return
        setIsUploading(true)
        for (const upload of waitingUploads) {
            updateUpload(upload.id, { status: 'uploading', error: null })
            try { updateUpload(upload.id, { status: 'uploaded', assetId: await uploader.uploadAsset(upload.file, upload.kind, upload.name.trim(), shouldPublish) }) }
            catch (error) { updateUpload(upload.id, { status: 'failed', error: error instanceof Error ? error.message : String(error) }) }
        }
        setIsUploading(false)
    }
    const isDisabled = uploader === null || isUploading
    return <div className="grid gap-2">
        <EditorFileDropZone label="Drop .glb, images, sounds, or folders, or click to browse" accept={ASSET_FILE_TYPES} isMultiple isDisabled={isDisabled} onFiles={addFiles} className={mainDropZoneClass} />
        <p className="text-[11px]/4 text-gray-500">Up to 16 MB per file. Images up to 2048 x 2048.</p>
        {skippedFileCount > 0 && <p className="text-[11px]/4 text-gray-500">Skipped {skippedFileCount === 1 ? '1 file that is not' : `${skippedFileCount} files that are not`} a .glb model, image, or sound.</p>}
        {uploads.length > 0 && <ul aria-label="Files to import" className="grid gap-1.5">
            {uploads.map(upload => <li key={upload.id} className="grid gap-0.5">
                <div className="flex min-h-7 items-center gap-1.5">
                    <span className="w-12 shrink-0 text-[11px] text-gray-500">{upload.kind}</span>
                    {upload.status === 'uploaded' ? <UploadName name={upload.name} assetId={upload.assetId} /> : <Input value={upload.name} onChange={event => updateUpload(upload.id, { name: event.target.value })} disabled={isUploading} aria-label={`Name for ${upload.file.name}`} autoComplete="off" data-lpignore="true" className="h-7 min-w-0 flex-1 text-xs" />}
                    {upload.status === 'uploading' && <LoaderCircle aria-label="Importing" className="size-3.5 shrink-0 animate-spin text-gray-400" />}
                    {upload.status === 'uploaded' && <Check aria-label="Imported" className="size-3.5 shrink-0 text-emerald-600" />}
                    {upload.status !== 'uploading' && <Button type="button" variant="ghost" size="icon" aria-label={`Remove ${upload.file.name}`} disabled={isUploading} className="size-5 rounded-sm text-gray-400 hover:text-gray-800 [&_svg]:size-3" onClick={() => setUploads(previous => previous.filter(item => item.id !== upload.id))}><X /></Button>}
                </div>
                {upload.error !== null && <p role="alert" className="break-words text-[11px]/4 text-red-600">{upload.error}</p>}
            </li>)}
        </ul>}
        <UploadToAssetsCheckbox shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} />
        <Button type="button" size="sm" disabled={isDisabled || waitingUploads.length === 0 || waitingUploads.some(upload => upload.name.trim() === '')} onClick={() => void uploadAll()}>{isUploading && <LoaderCircle className="animate-spin" />}{waitingUploads.length === 0 ? 'Import' : `Import ${waitingUploads.length === 1 ? '1 file' : `${waitingUploads.length} files`}`}</Button>
    </div>
}

// A material uploads one image for each of its maps together, named after its color texture, which is the only
// required one. A failed upload keeps its images in the form so it can be sent again.
const MaterialUploadForm = ({ uploader, shouldPublish, setShouldPublish }: { uploader: EditorAssetUploader | null, shouldPublish: boolean, setShouldPublish: (shouldPublish: boolean) => void }) => {
    const [textureFileOfSlot, setTextureFileOfSlot] = useState(emptyMaterialTextureFileOfSlot)
    const [uploads, setUploads] = useState<ImportedUpload[]>([])
    const [error, setError] = useState<string | null>(null)
    const chooseTextureFile = (slot: MaterialTextureSlot, [file]: File[]) => {
        if (file) setTextureFileOfSlot(previous => ({ ...previous, [slot]: file }))
    }
    const isUploading = uploads.some(upload => upload.status === 'uploading')
    const uploadMaterial = async (uploader: EditorAssetUploader, albedo: File) => {
        const id = crypto.randomUUID()
        const name = getNameOfFile(albedo)
        setError(null)
        setUploads(previous => [...previous, { id, name, status: 'uploading', assetId: null }])
        try {
            const assetId = await uploader.uploadMaterial(name, textureFileOfSlot, shouldPublish)
            setUploads(previous => previous.map((upload): ImportedUpload => upload.id === id ? { ...upload, status: 'uploaded', assetId } : upload))
            setTextureFileOfSlot(emptyMaterialTextureFileOfSlot)
        } catch (error) {
            setUploads(previous => previous.filter(upload => upload.id !== id))
            setError(error instanceof Error ? error.message : String(error))
        }
    }
    // TODO: AI-PICKED-VALUE: Each map's drop zone is 80px tall, one per row.
    return <div className="grid gap-2">
        {materialTextures.map(({ slot, label, isRequired }) => <div key={slot} className="grid min-w-0 gap-1">
            <div className="flex min-w-0 items-center gap-1 text-[11px]/4">
                <span className="truncate font-medium text-gray-700">{label}</span>
                {!isRequired && <span className="text-gray-400">Optional</span>}
                {textureFileOfSlot[slot] !== null && <Button type="button" variant="ghost" size="icon" aria-label={`Remove ${label}`} className="ml-auto size-4 rounded-sm text-gray-400 hover:text-gray-800 [&_svg]:size-3" onClick={() => setTextureFileOfSlot(previous => ({ ...previous, [slot]: null }))}><X /></Button>}
            </div>
            <EditorFileDropZone label={textureFileOfSlot[slot]?.name ?? 'Drop an image, or click to browse'} accept={IMAGE_FILE_TYPES} isMultiple={false} isDisabled={uploader === null} onFiles={files => chooseTextureFile(slot, files)} className="h-20 flex-col justify-center text-center" />
        </div>)}
        <p className="text-[11px]/4 text-gray-500">Each image up to 2048 x 2048.</p>
        <UploadToAssetsCheckbox shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} />
        <Button type="button" size="sm" disabled={uploader === null || textureFileOfSlot.albedo === null || isUploading} onClick={() => { if (uploader && textureFileOfSlot.albedo) void uploadMaterial(uploader, textureFileOfSlot.albedo) }}>{isUploading && <LoaderCircle className="animate-spin" />}Import</Button>
        {error !== null && <p role="alert" className="break-words text-[11px]/4 text-red-600">{error}</p>}
        <ImportedUploadList label="Imported materials" uploads={uploads} />
    </div>
}

// A skybox is one panorama image, named after its file. A failed upload keeps the image in the form so it can be sent again.
const SkyboxUploadForm = ({ uploader, shouldPublish, setShouldPublish }: { uploader: EditorAssetUploader | null, shouldPublish: boolean, setShouldPublish: (shouldPublish: boolean) => void }) => {
    const [file, setFile] = useState<File | null>(null)
    const [uploads, setUploads] = useState<ImportedUpload[]>([])
    const [error, setError] = useState<string | null>(null)
    const isUploading = uploads.some(upload => upload.status === 'uploading')
    const uploadSkybox = async (uploader: EditorAssetUploader, file: File) => {
        const id = crypto.randomUUID()
        const name = getNameOfFile(file)
        setError(null)
        setUploads(previous => [...previous, { id, name, status: 'uploading', assetId: null }])
        try {
            const assetId = await uploader.uploadAsset(file, 'Skybox', name, shouldPublish)
            setUploads(previous => previous.map((upload): ImportedUpload => upload.id === id ? { ...upload, status: 'uploaded', assetId } : upload))
            setFile(null)
        } catch (error) {
            setUploads(previous => previous.filter(upload => upload.id !== id))
            setError(error instanceof Error ? error.message : String(error))
        }
    }
    return <div className="grid gap-2">
        <EditorFileDropZone label={file?.name ?? 'Drop a panorama image, or click to browse'} accept={IMAGE_FILE_TYPES} isMultiple={false} isDisabled={uploader === null} onFiles={([droppedFile]) => { if (droppedFile) setFile(droppedFile) }} className={mainDropZoneClass} />
        <p className="text-[11px]/4 text-gray-500">Twice as wide as it is tall, up to 4096 x 2048.</p>
        <UploadToAssetsCheckbox shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} />
        <Button type="button" size="sm" disabled={uploader === null || file === null || isUploading} onClick={() => { if (uploader && file) void uploadSkybox(uploader, file) }}>{isUploading && <LoaderCircle className="animate-spin" />}Import</Button>
        {error !== null && <p role="alert" className="break-words text-[11px]/4 text-red-600">{error}</p>}
        <ImportedUploadList label="Imported skyboxes" uploads={uploads} />
    </div>
}

// The Import panel adds files to the world, and also publishes them as assets while Upload to Assets is checked. Models,
// images, and sounds upload together
// as separate files; a material is several images that become one asset, so it has its own tab, and so does a skybox, an
// image that becomes the sky. The tabs stay mounted, so switching keeps what was added.
export const EditorUploadPanel = ({ uploader, onClose }: { uploader: EditorAssetUploader | null, onClose: () => void }) => {
    const [shouldPublish, setShouldPublish] = useState(true)
    return <section aria-label="Import" className="flex h-full min-h-0 flex-col">
        <EditorSidePanelHeader title="Import" onClose={onClose} />
        <Tabs defaultValue="files" className="min-h-0 flex-1 gap-3 overflow-y-auto px-3 pb-3">
            <TabsList className="w-full">
                <TabsTrigger value="files" className="text-xs">Mesh, Texture, Sound</TabsTrigger>
                <TabsTrigger value="material" className="text-xs">Material</TabsTrigger>
                <TabsTrigger value="skybox" className="text-xs">Skybox</TabsTrigger>
            </TabsList>
            <TabsContent value="files" forceMount className="data-[state=inactive]:hidden"><AssetFilesUploadForm uploader={uploader} shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} /></TabsContent>
            <TabsContent value="material" forceMount className="data-[state=inactive]:hidden"><MaterialUploadForm uploader={uploader} shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} /></TabsContent>
            <TabsContent value="skybox" forceMount className="data-[state=inactive]:hidden"><SkyboxUploadForm uploader={uploader} shouldPublish={shouldPublish} setShouldPublish={setShouldPublish} /></TabsContent>
        </Tabs>
    </section>
}
