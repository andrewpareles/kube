'use client'

import Link from 'next/link'
import dynamic from 'next/dynamic'
import { createContext, memo, useCallback, useContext, useEffect, useEffectEvent, useLayoutEffect, useMemo, useReducer, useRef, useState, useSyncExternalStore, type ComponentProps, type ReactNode } from 'react'
import { ArrowLeft, ArrowUp, Camera, Check, CloudUpload, EllipsisVertical, Globe2, Hammer, Lock, ExternalLink, Layers, LoaderCircle, Play, Redo2, Settings, Shapes, Sparkles, Square, Undo2 } from 'lucide-react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuShortcut, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'
import { detectPlatform, formatForDisplay } from '@tanstack/hotkeys'
import { publishGame, unpublishGame } from '@/app/server-actions/edgegap-actions'
import { TbFileDownload, TbFileUpload } from 'react-icons/tb'
import { IoChatbubbleOutline } from 'react-icons/io5'
import { HoverTooltip } from '@/components/site/HoverTooltip'
import { Skeleton } from '@/shadcn/components/ui/skeleton'
import { getToolOrDynamicToolName, isToolUIPart, type ReasoningUIPart, type TextUIPart, type UIMessage } from 'ai'
import { z } from 'zod'
import { Streamdown } from 'streamdown'
import { toast } from 'sonner'
import { chatMarkdownRehypePlugins } from '@/lib/markdownRehypePlugins'
import { GAME_PATH } from '@/app/deeplink/deeplinkFlowConsts'
// import { EDITOR_PATH } from '@/lib/siteNavigation'
// import { useCredits } from '@/components/context/useUser'
// import { AccountBalance } from '@/components/site/SiteHeader'
import { editorHeaders, editorRequest, editorUploadRequest, readEditorSessionHash } from '../editorApi'
import { supabaseOnClient } from '@/supabase/supabaseOnClient'
import { confirmEditorLeave, listenForEditorLeave, openAppLink } from '../editorLeaveWarning'
import { watchBrowserAfterDeepLink } from '@/app/deeplink/sendDeepLinks'
import { DeeplinkFeedbackModal } from '@/components/site/DeeplinkFeedbackModal'
import { GameSettingsDialog } from '@/app/games/[game_id]/GameSettingsDialog'
import { useDeeplinkFeedback } from '@/components/site/useDeeplinkFeedback'
import { EditorScene, EditorThumbnail } from './EditorScene'
import { EditorObjectPanel } from './EditorObjectPanel'
import { EditorObjectIcon } from './EditorObjectIcon'
import { EditorPanels } from './EditorPanels'
import { EditorChatScroll } from './EditorChatScroll'
import { EditorDocumentTabs } from './EditorDocumentTabs'
import { EditorShare } from './EditorShare'
import { EditorModelPicker, useEditorModel } from './EditorModelPicker'
import { EditorAssetsPanel } from './EditorAssetsPanel'
import { EditorWorldFilePanel } from './EditorWorldFilePanel'
import { EditorUploadPanel, type EditorAssetKind, type MaterialTextureFileOfSlot } from './EditorUploadPanel'
import type { BrowserEditorWorld } from '@/lib/gen/render_web/render_web'
import { initializeWgpuRenderer } from '@/lib/rendering/initializeWgpuRenderer'
import { getObjectKind, objectInfoSchemaOfKind, storedWorldSchema, type StoredWorld } from '../editorObjectSchema'
import { createEditorWorldState, editorWorldReducer } from '../editorWorld'
import { getEditorHistoryDirection, getEditorSelectionShortcut } from '../editorHistory'
import { getEditorCameraOfLocation, getLocationOfEditorCamera, type EditorCamera } from '../editorCamera'
import { applyBrowserEditorDelta, browserEditorDeltaSchema, createEditorClientActionSender, editorStateSchema, type EditorClientAction, type EditorState } from '../editorSync'
import { scriptTypeEnvironmentSchema, type ScriptServices } from '../editorScripts'
import { createEditorChatMessage } from '../editorChat'
import { useEditorChat } from './useEditorChat'
import { useEditorScripts } from './useEditorScripts'

const EditorScriptEditor = dynamic(() => import('./EditorScriptEditor').then(module => module.EditorScriptEditor), { ssr: false, loading: () => <div role="status" className="p-4 text-xs text-gray-500">Loading script editor…</div> })

const connectionSchema = z.object({ editor_url: z.url(), chat_url: z.url() })
const publishResultSchema = z.object({ is_error: z.boolean(), text: z.string() })
const publishedLinkShownMs = 6000
// TODO: AI-PICKED-VALUE: The editor's menu button shows a check for a moment after one of its options saves.
const menuCheckShownMs = 6000
type OpenedEditor = z.infer<typeof connectionSchema> & EditorState
const addedObjectSchema = z.object({ object_id: z.number() })
const uploadedObjectSchema = z.object({ object_id: z.number(), asset_id: z.number().nullable() })
const chatActionClass = 'inline-flex size-8 shrink-0 items-center justify-center rounded-full'
const historyButtonClass = 'rounded-lg p-2 text-gray-500 hover:bg-gray-100 disabled:opacity-40 disabled:hover:bg-transparent'
const buttonClass = 'inline-flex items-center justify-center gap-2 rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm hover:bg-gray-50 disabled:cursor-default disabled:opacity-50'
const primaryButtonClass = 'inline-flex items-center justify-center gap-2 rounded-lg border border-primary bg-primary px-3 py-2 text-sm text-primary-foreground hover:bg-primary/90 disabled:cursor-default disabled:opacity-50'
const mobileViewButtonClass = `absolute bottom-3 right-3 z-10 shadow-md ${buttonClass}`
// TODO: AI-PICKED-VALUE: The labelled header buttons wait 700 ms before showing their tooltips.
const bigButtonTooltipDelayMs = 700
const firstDraftButtonTooltip = 'Just a few minutes!'
const backLinkClass = '-mx-2 flex min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 text-sm font-semibold'
const panelToggleClass = 'shrink-0 rounded-lg p-2 text-gray-700 hover:bg-gray-100 aria-pressed:bg-gray-100'
type SidePanel = 'assets' | 'upload' | 'download' | null
// The editor uses its phone layout below Tailwind's lg breakpoint (1024px).
const mobileMediaQuery = '(width < 64rem)'
const subscribeToMobileMediaQuery = (onChange: () => void) => {
    const query = matchMedia(mobileMediaQuery)
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
}
const useIsMobile = () => useSyncExternalStore(subscribeToMobileMediaQuery, () => matchMedia(mobileMediaQuery).matches, () => false)

// Reloading joins the current editor and its saved conversation; no game state is kept in browser storage.
export const BrowserEditor = ({ gameId, gameName, gameDescription, gameSuggestedMinPlayers, gameMaxPlayers, thumbnailUuid, isCreator, isGamePublished, isGamePublic, inviteToken }: { gameId: number, gameName: string, gameDescription: string | null, gameSuggestedMinPlayers: number, gameMaxPlayers: number, thumbnailUuid: string | null, isCreator: boolean, isGamePublished: boolean, isGamePublic: boolean, inviteToken: string | null }) => {
    const [opened, setOpened] = useState<OpenedEditor | null>(null)
    const [didFail, setDidFail] = useState(false)
    // Why a connected editor went away, shown until the person starts editing again.
    const [closedReason, setClosedReason] = useState<string | null>(null)
    const [attempt, setAttempt] = useState(0)
    const [isShareOpen, setIsShareOpen] = useState(false)
    // Kept here so a panel opened while the editor starts stays open once it connects.
    const [sidePanel, setSidePanel] = useState<SidePanel>(null)
    const [isExplorerShown, setIsExplorerShown] = useState(false)
    // TODO: AI-PICKED-VALUE: Phones show the world preview first, so its loading screen is what people see while the editor starts, and switch to the chat from the button floating in its bottom right.
    const [isMobilePreviewShown, setIsMobilePreviewShown] = useState(true)
    const isMobile = useIsMobile()
    useEffect(() => {
        const controller = new AbortController()
        let socket: WebSocket | undefined
        let streamedWorld: BrowserEditorWorld | undefined
        const open = async () => {
            setDidFail(false)
            setClosedReason(null)
            // TODO: AI-PICKED-VALUE: Allow five minutes for an Edgegap cold start, polling every two seconds.
            for (let poll = 0; poll < 150; poll++) {
                const response = await editorRequest(inviteToken, '', `/api/edgegap-editor/start-editor-server/${gameId}`, 'POST', undefined, controller.signal)
                if (response.status !== 202) {
                    const connection = connectionSchema.parse(await response.json())
                    const [renderWeb, { Authorization }] = await Promise.all([initializeWgpuRenderer(), editorHeaders(inviteToken)])
                    if (controller.signal.aborted) return
                    const url = new URL(`${connection.editor_url}/editor-server-websocket`)
                    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
                    const connectionSocket = new WebSocket(url)
                    socket = connectionSocket
                    connectionSocket.binaryType = 'arraybuffer'
                    connectionSocket.onopen = () => connectionSocket.send(Authorization)
                    // Keep batches in order while new meshes and textures load. Rust applies the object changes.
                    // The stream belongs to the workspace, including when a script tab hides the canvas.
                    connectionSocket.onmessage = (event: MessageEvent<string | ArrayBuffer>) => {
                        if (controller.signal.aborted) return
                        try {
                            if (typeof event.data === 'string') {
                                if (streamedWorld) throw new Error('Received a second initial editor state.')
                                const state = editorStateSchema.parse(JSON.parse(event.data))
                                streamedWorld = renderWeb.BrowserEditorWorld.from_json(event.data)
                                setOpened({ ...connection, ...state })
                            } else {
                                if (!streamedWorld) throw new Error('Received editor changes before the initial state.')
                                const delta = browserEditorDeltaSchema.parse(JSON.parse(streamedWorld.apply_deltas(new Uint8Array(event.data))))
                                setOpened(previous => previous ? applyBrowserEditorDelta(previous, delta) : previous)
                            }
                        } catch (error) {
                            connectionSocket.onclose = null
                            connectionSocket.close()
                            setDidFail(true)
                            toast.error(String(error), { id: 'editor-world-connection' })
                        }
                    }
                    // The editor server says why it closed, such as pausing after nobody used it for a while.
                    connectionSocket.onclose = event => {
                        if (controller.signal.aborted) return
                        setOpened(null)
                        // A working connection that closes with no reason was cut off, like when a phone puts the page in the background, so the
                        // editor opens again by itself. A hidden tab waits until it is shown, so it never starts an editor nobody is looking at.
                        if (streamedWorld && !event.reason) {
                            const reopen = () => { if (document.visibilityState === 'visible') setAttempt(value => value + 1) }
                            document.addEventListener('visibilitychange', reopen, { signal: controller.signal })
                            reopen()
                            return
                        }
                        setClosedReason(event.reason || 'Editor connection closed')
                    }
                    // Anything the person does anywhere on the page keeps the editor server running.
                    // TODO: AI-PICKED-VALUE: Report activity at most once every 30 seconds.
                    let lastActivitySentAt = 0
                    const sendActivity = () => {
                        if (connectionSocket.readyState !== WebSocket.OPEN || Date.now() - lastActivitySentAt < 30_000) return
                        lastActivitySentAt = Date.now()
                        connectionSocket.send('activity')
                    }
                    for (const type of ['pointerdown', 'pointermove', 'keydown', 'wheel']) window.addEventListener(type, sendActivity, { capture: true, passive: true, signal: controller.signal })
                    return
                }
                await new Promise<void>(resolve => setTimeout(resolve, 2000))
                controller.signal.throwIfAborted()
            }
            throw new Error('The editor is taking too long to start. Try again.')
        }
        void open().catch(error => { if (!controller.signal.aborted) { setDidFail(true); toast.error(error instanceof Error ? error.message : String(error)) } })
        return () => { controller.abort(); socket?.close(); streamedWorld?.free() }
    }, [gameId, attempt, inviteToken])
    if (opened) return <EditorWorkspace isCreator={isCreator} isGamePublished={isGamePublished} isGamePublic={isGamePublic} inviteToken={inviteToken} gameId={gameId} gameName={gameName} gameDescription={gameDescription} gameSuggestedMinPlayers={gameSuggestedMinPlayers} gameMaxPlayers={gameMaxPlayers} thumbnailUuid={thumbnailUuid} opened={opened} sidePanel={sidePanel} setSidePanel={setSidePanel} isExplorerShown={isExplorerShown} setIsExplorerShown={setIsExplorerShown} isMobilePreviewShown={isMobilePreviewShown} setIsMobilePreviewShown={setIsMobilePreviewShown} />
    return <EditorLayout
        onLeave={undefined}
        isCreator={isCreator}
        isGamePublic={isGamePublic}
        gameId={gameId}
        gameName={gameName}
        sidePanel={sidePanel}
        setSidePanel={setSidePanel}
        isExplorerShown={isExplorerShown}
        setIsExplorerShown={setIsExplorerShown}
        isMobilePreviewShown={isMobilePreviewShown}
        setIsMobilePreviewShown={setIsMobilePreviewShown}
        assets={null}
        upload={null}
        worldFile={null}
        inspector={<EditorObjectPanel editor={null} />}
        isMenuCheckShown={false}
        menuItems={null}
        controls={<>
            {isCreator && <EditorShare gameId={gameId} isOpen={isShareOpen} setIsOpen={setIsShareOpen} />}
            {!isMobile && <button className={primaryButtonClass} disabled><Play className="size-4 fill-current" />Playtest</button>}
        </>}
        chatMessages={<>
            <div role="status" aria-label="Loading chat" className="min-h-0 flex-1 space-y-3 overflow-hidden px-6 pt-3">
                <ChatMessageSkeleton />
                <ChatMessageSkeleton />
            </div>
        </>}
        chatComposer={<>
            <div className="mx-3 mb-3 shrink-0 rounded-xl border border-gray-200 px-3 pt-2 pb-3 shadow-sm">
                <textarea aria-label="Message" autoComplete="off" data-lpignore="true" disabled placeholder="Describe what you want to build…" rows={3} className="w-full resize-none bg-transparent text-[13px]/5 outline-none placeholder:text-gray-400" />
                <div className="flex justify-end"><button aria-label="Send message" disabled className={`${chatActionClass} bg-gray-950 text-white disabled:opacity-30`}><ArrowUp className="size-4" /></button></div>
            </div>
        </>}
    >
        <div aria-busy={!didFail && closedReason === null} className="editor-loading-sky relative flex min-h-0 min-w-0 flex-1 items-center justify-center">
            <EditorThumbnail thumbnailUuid={thumbnailUuid} isLoading={!didFail && closedReason === null} />
            {didFail && <button className={`relative ${buttonClass}`} onClick={() => setAttempt(value => value + 1)}>Try Again</button>}
            {closedReason !== null && <div className="relative flex flex-col items-center gap-3 rounded-xl border border-gray-200 bg-white px-6 py-5 shadow-sm">
                <p className="text-sm font-medium">{closedReason}</p>
                <button className={buttonClass} onClick={() => setAttempt(value => value + 1)}>Start Editing</button>
            </div>}
        </div>
    </EditorLayout>
}

// Loading and connected editors share the same layout so the workspace is visible during startup.
// What the chat looks like while the editor loads: a message someone sent, then the reply under it.
const ChatMessageSkeleton = () => <div aria-hidden="true" className="space-y-3">
    <Skeleton className="bg-gray-100 h-9 rounded-lg" />
    <Skeleton className="bg-gray-100 h-16 rounded-lg" />
</div>
const formatShortcut = (hotkey: string) => formatForDisplay(hotkey, { separatorToken: detectPlatform() === 'mac' ? '' : null })
const EditorMenuItems = ({ canUndo, canRedo, onHistoryStep, isPlaytestRunning, onSetFallbackCamera, onRefreshAiThumbnail, isPublished, onToggleIsPublished, onOpenGameSettings }: { canUndo: boolean, canRedo: boolean, onHistoryStep: (direction: 'undo' | 'redo') => void, isPlaytestRunning: boolean, onSetFallbackCamera: () => void, onRefreshAiThumbnail: () => void, isPublished: boolean, onToggleIsPublished: () => void, onOpenGameSettings: () => void }) => <>
    <DropdownMenuItem disabled={!canUndo} onClick={() => onHistoryStep('undo')}><Undo2 className="size-4" />Undo<DropdownMenuShortcut>{formatShortcut('Mod+Z')}</DropdownMenuShortcut></DropdownMenuItem>
    <DropdownMenuItem disabled={!canRedo} onClick={() => onHistoryStep('redo')}><Redo2 className="size-4" />Redo<DropdownMenuShortcut>{formatShortcut('Mod+Shift+Z')}</DropdownMenuShortcut></DropdownMenuItem>
    {!isPlaytestRunning && <>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={onOpenGameSettings}><Settings className="size-4" />Game Settings</DropdownMenuItem>
        <DropdownMenuItem onClick={onToggleIsPublished}>{isPublished ? <Lock className="size-4" /> : <Globe2 className="size-4" />}Change to {isPublished ? 'Private' : 'Public'}</DropdownMenuItem>
        <DropdownMenuItem onClick={onSetFallbackCamera}><Camera className="size-4" />Set Thumbnail Angle</DropdownMenuItem>
        <DropdownMenuItem onClick={onRefreshAiThumbnail}><Sparkles className="size-4" />Refresh AI Thumbnail</DropdownMenuItem>
    </>}
</>
const PanelToggle = ({ label, isShown, onToggle, children }: { label: string, isShown: boolean, onToggle: () => void, children: ReactNode }) =>
    <HoverTooltip side="bottom" className="" trigger={<button type="button" aria-label={label} aria-pressed={isShown} className={panelToggleClass} onClick={onToggle}>{children}</button>}>{label}</HoverTooltip>
const EditorLayout = ({ gameId, gameName, isCreator, isGamePublic, sidePanel, setSidePanel, isExplorerShown, setIsExplorerShown, isMobilePreviewShown, setIsMobilePreviewShown, isMenuCheckShown, menuItems, controls, chatMessages, chatComposer, assets, upload, worldFile, inspector, children, onLeave }: { gameId: number, gameName: string, isCreator: boolean, isGamePublic: boolean, sidePanel: SidePanel, setSidePanel: (sidePanel: SidePanel) => void, isExplorerShown: boolean, setIsExplorerShown: (isExplorerShown: boolean) => void, isMobilePreviewShown: boolean, setIsMobilePreviewShown: (isMobilePreviewShown: boolean) => void, isMenuCheckShown: boolean, menuItems: ReactNode, controls: ReactNode, chatMessages: ReactNode, chatComposer: ReactNode, assets: ComponentProps<typeof EditorAssetsPanel>['editor'], upload: ComponentProps<typeof EditorUploadPanel>['uploader'], worldFile: ComponentProps<typeof EditorWorldFilePanel>['editor'], inspector: ReactNode, children: ReactNode, onLeave: ((event: { preventDefault: () => void }) => void) | undefined }) => {
    const isMobile = useIsMobile()
    // const credits = useCredits()
    const closeSidePanel = useCallback(() => setSidePanel(null), [setSidePanel])
    const sidePanelContent = sidePanel === 'assets' ? <EditorAssetsPanel editor={assets} onClose={closeSidePanel} /> : sidePanel === 'upload' ? <EditorUploadPanel uploader={upload} onClose={closeSidePanel} /> : sidePanel === 'download' ? <EditorWorldFilePanel editor={worldFile} onClose={closeSidePanel} /> : null
    return <main className="editor-scrollbars flex h-dvh flex-col bg-white">
        <header className="editor-header relative flex items-center gap-3 border-b border-gray-200 px-4 py-3">
            {!isMobile && <h1 className="min-w-0">
                {isCreator || isGamePublic
                    ? <HoverTooltip side="bottom" className="" trigger={<Link href={GAME_PATH(gameId)} onNavigate={onLeave} className={backLinkClass}><ArrowLeft className="size-4 shrink-0 text-gray-500" /><span className="truncate">{gameName}</span></Link>}>Back to Game Page</HoverTooltip>
                    : <HoverTooltip side="bottom" className="" trigger={<span role="link" aria-disabled="true" tabIndex={0} className={`${backLinkClass} cursor-default`}><ArrowLeft className="size-4 shrink-0 text-gray-500 opacity-40" /><span className="truncate">{gameName}</span></span>}>The game you&apos;re editing is private</HoverTooltip>}
            </h1>}
            {(isMobile || menuItems) && <DropdownMenu modal={false}>
                <DropdownMenuTrigger asChild>
                    <button type="button" aria-label="More options" className={historyButtonClass}>{isMenuCheckShown ? <Check className="size-4 text-emerald-600" /> : <EllipsisVertical className="size-4" />}</button>
                </DropdownMenuTrigger>
                <DropdownMenuContent side="bottom" align="center" onCloseAutoFocus={event => event.preventDefault()}>
                    {isMobile && <>
                        <DropdownMenuItem onClick={() => setIsMobilePreviewShown(!isMobilePreviewShown)}>{isMobilePreviewShown ? <><IoChatbubbleOutline className="size-4" />Show Chat</> : <><Hammer className="size-4" />Show Editor</>}</DropdownMenuItem>
                        {(isCreator || isGamePublic) && <DropdownMenuItem asChild><Link href={GAME_PATH(gameId)} onNavigate={onLeave}><ArrowLeft className="size-4" />View Game Page</Link></DropdownMenuItem>}
                        {menuItems && <DropdownMenuSeparator />}
                    </>}
                    {menuItems}
                </DropdownMenuContent>
            </DropdownMenu>}
            {/* {credits !== null && <AccountBalance credits={credits} nextPath={EDITOR_PATH(gameId)} onNavigate={onLeave} />} */}
            <div className="flex-1" />
            {!isMobile && <div className="absolute left-1/2 flex -translate-x-1/2 items-center gap-2">
                <PanelToggle label="Explorer" isShown={isExplorerShown} onToggle={() => setIsExplorerShown(!isExplorerShown)}><Layers className="size-4" /></PanelToggle>
                <PanelToggle label="Assets" isShown={sidePanel === 'assets'} onToggle={() => setSidePanel(sidePanel === 'assets' ? null : 'assets')}><Shapes className="size-4" /></PanelToggle>
                <PanelToggle label="Import" isShown={sidePanel === 'upload'} onToggle={() => setSidePanel(sidePanel === 'upload' ? null : 'upload')}><TbFileUpload className="size-4" /></PanelToggle>
                <PanelToggle label="Download" isShown={sidePanel === 'download'} onToggle={() => setSidePanel(sidePanel === 'download' ? null : 'download')}><TbFileDownload className="size-4" /></PanelToggle>
            </div>}
            <div className="flex min-w-0 flex-wrap items-center justify-end gap-2">{controls}</div>
        </header>
        <div className="min-h-0 flex-1">
            {isMobile
                ? <div className="flex h-full flex-col">
                    <aside aria-label="AI chat" className={`min-h-0 min-w-0 flex-1 flex-col ${isMobilePreviewShown ? 'hidden' : 'flex'}`}>
                        <div className="relative flex min-h-0 flex-1 flex-col">{chatMessages}<button type="button" className={mobileViewButtonClass} onClick={() => setIsMobilePreviewShown(true)}><Hammer className="size-4" />Show Editor</button></div>
                        {chatComposer}
                    </aside>
                    <div className={`relative min-h-0 min-w-0 flex-1 flex-col ${isMobilePreviewShown ? 'flex' : 'hidden'}`}>{children}<button type="button" className={mobileViewButtonClass} onClick={() => setIsMobilePreviewShown(false)}><IoChatbubbleOutline className="size-4" />Show Chat</button></div>
                </div>
                : <EditorPanels chat={<>{chatMessages}{chatComposer}</>} sidePanel={sidePanelContent} inspector={isExplorerShown ? inspector : null}>{children}</EditorPanels>}
        </div>
    </main>
}

const EditorWorkspace = ({ gameId, gameName, gameDescription, gameSuggestedMinPlayers, gameMaxPlayers, thumbnailUuid, opened, isCreator, isGamePublished, isGamePublic, inviteToken, sidePanel, setSidePanel, isExplorerShown, setIsExplorerShown, isMobilePreviewShown, setIsMobilePreviewShown }: { gameId: number, gameName: string, gameDescription: string | null, gameSuggestedMinPlayers: number, gameMaxPlayers: number, thumbnailUuid: string | null, opened: OpenedEditor, isCreator: boolean, isGamePublished: boolean, isGamePublic: boolean, inviteToken: string | null, sidePanel: SidePanel, setSidePanel: (sidePanel: SidePanel) => void, isExplorerShown: boolean, setIsExplorerShown: (isExplorerShown: boolean) => void, isMobilePreviewShown: boolean, setIsMobilePreviewShown: (isMobilePreviewShown: boolean) => void }) => {
    const [editorState, dispatchEditor] = useReducer(editorWorldReducer, opened.world, createEditorWorldState)
    const [isShareOpen, setIsShareOpen] = useState(false)
    const isMobile = useIsMobile()
    useEffect(() => { dispatchEditor({ type: 'refresh', world: opened.world, seq: opened.seq }) }, [opened.world, opened.seq])
    const clientActionSender = useMemo(() => createEditorClientActionSender(async action => {
        const response = await editorRequest(inviteToken, opened.editor_url, '/editor-client-action', 'POST', action, undefined)
        return z.object({ seq: z.number() }).parse(await response.json()).seq
    }), [opened.editor_url, inviteToken])
    useEffect(() => listenForEditorLeave(), [])
    // The script editor loads while the network is still there, so opening a script offline cannot fail to load it.
    useEffect(() => { import('./EditorScriptEditor').catch(() => { }) }, [])
    // The reducer validates previews; the server validates requests before changing the shared world.
    // Every delta the reducer applied leaves here in order, so a burst of edits or undo shortcuts
    // can never send a step the preview did not apply. What was sent is remembered here rather than
    // in the reducer, so sending does not render the whole editor a second time.
    const sentActionsRef = useRef(new WeakSet<EditorClientAction>())
    useEffect(() => {
        for (const action of editorState.actionsToSend) {
            if (sentActionsRef.current.has(action)) continue
            sentActionsRef.current.add(action)
            void clientActionSender.send(action).then(seq => dispatchEditor({ type: 'acked', action, seq }), error => {
                // The editor server rejected it, so the preview goes back to the world it still owns.
                dispatchEditor({ type: 'rejected', action })
                toast.error(error instanceof Error ? error.message : String(error), { id: 'editor-property-save-error' })
            })
        }
    }, [editorState.actionsToSend, clientActionSender])
    // The whole workspace shares one shortcut, so undo works from the viewport, explorer, and inspector.
    // The editor server owns the history: it applies the step and broadcasts the result like any edit,
    // and it selects what moved, so a step on an object nobody is looking at can still be found.
    // The editor server says whether this session has anything to undo or redo.
    const [sessionHash, setSessionHash] = useState<string | null>(null)
    useEffect(() => { void readEditorSessionHash().then(setSessionHash) }, [])
    const historyAvailability = sessionHash === null ? undefined : opened.editor_status.history_of_session[sessionHash]
    const canUndo = historyAvailability?.can_undo ?? false
    const canRedo = historyAvailability?.can_redo ?? false
    // A spinner shows from the press until the answer, so a step that takes a moment still feels taken.
    const [pendingHistoryStepCount, setPendingHistoryStepCount] = useState(0)
    const takeHistoryStep = useCallback((direction: 'undo' | 'redo') => {
        setPendingHistoryStepCount(count => count + 1)
        void (async () => {
            const response = await editorRequest(inviteToken, opened.editor_url, `/editor-history/${direction}`, 'POST', undefined, undefined)
            const { object_ids } = z.object({ object_ids: z.array(z.number()).optional() }).parse(await response.json())
            if (object_ids?.length) dispatchEditor({ type: 'select-objects', objectIds: object_ids })
        })()
            .catch(error => toast.error(error instanceof Error ? error.message : String(error), { id: 'editor-history' }))
            .finally(() => setPendingHistoryStepCount(count => count - 1))
    }, [inviteToken, opened.editor_url])
    const handleShortcutKey = useEffectEvent((event: KeyboardEvent) => {
        const direction = getEditorHistoryDirection(event)
        if (direction) {
            event.preventDefault()
            if (direction === 'undo' ? canUndo : canRedo) takeHistoryStep(direction)
            return
        }
        const shortcut = getEditorSelectionShortcut(event)
        if (shortcut === 'select-root') {
            event.preventDefault()
            dispatchEditor({ type: 'select', objectId: editorState.world.rootObjectId })
            setFrameRequest(null)
        }
        const objectId = editorState.selectedObjectIds.at(-1)
        if (shortcut === 'frame' && objectId !== undefined && editorState.activeScriptId === null) {
            event.preventDefault()
            setFrameRequest({ objectId })
        }
    })
    useEffect(() => {
        window.addEventListener('keydown', handleShortcutKey)
        return () => window.removeEventListener('keydown', handleShortcutKey)
    }, [])
    const scriptServices = useMemo<ScriptServices>(() => ({
        readTypes: async signal => scriptTypeEnvironmentSchema.parse(await (await editorRequest(inviteToken, opened.chat_url, '/get-script-types', 'GET', undefined, signal)).json()),
    }), [opened.chat_url, inviteToken])
    const scripts = useEditorScripts(editorState, scriptServices)
    const world = useMemo(() => JSON.stringify(editorState.world), [editorState.world])
    const [frameRequest, setFrameRequest] = useState<{ objectId: number } | null>(null)
    // The composer shows the same selection as the inspector and viewport.
    const selectedObjects = editorState.selectedObjectIds.map(id => editorState.world.objectOfId[id])
    // A string, so chat messages only re-render when the selection changes. A turn's row only matches its
    // object if the name and kind match too, because older rows and ids freed by a delete can point elsewhere.
    const selectedObjectKeys = JSON.stringify(selectedObjects.map(object => [object.id, object.name, getObjectKind(object)]))
    const { is_playtest_running: isPlaytestRunning, is_chat_running: isChatRunning } = opened.editor_status
    const [isPlaytestPending, setIsPlaytestPending] = useState(false)
    const [input, setInput] = useState('')
    const focusScriptRef = useRef<(() => void) | null>(null)
    // A listed object is checked when clicked, so chat messages do not re-render on every edit. An object
    // that is gone, or renamed since, is not selected.
    const objectOfIdRef = useRef(editorState.world.objectOfId)
    useEffect(() => { objectOfIdRef.current = editorState.world.objectOfId }, [editorState.world.objectOfId])
    const selectChangedObject = useCallback((change: ChatObject) => {
        const object = objectOfIdRef.current[change.objectId]
        if (object?.name !== change.name || getObjectKind(object) !== change.kind) return
        dispatchEditor({ type: 'select', objectId: change.objectId })
        setFrameRequest(null)
    }, [])
    // The editor opens at the game's fallback camera.
    const cameraRef = useRef<EditorCamera | null>(getEditorCameraOfLocation(opened.world.fallback_camera_location))
    // An uploaded world arrives over the WebSocket apart from the response, and a new world's camera frames
    // whatever is showing, so its camera waits for the world.
    const shownWorldRef = useRef(world)
    const uploadedCameraRef = useRef<{ camera: EditorCamera | null } | null>(null)
    useEffect(() => {
        shownWorldRef.current = world
        if (!uploadedCameraRef.current) return
        cameraRef.current = uploadedCameraRef.current.camera
        uploadedCameraRef.current = null
    }, [world])
    const [model, pickModel] = useEditorModel()
    // A rejected message comes back only into an empty box, so it never overwrites something typed since.
    const restoreChatInput = useCallback((text: string) => setInput(current => current.trim() === '' ? text : current), [])
    const { messages, sendMessage, isResponding, isChatConnected, isOutOfCredits, unsent } = useEditorChat(inviteToken, opened.chat_url, isChatRunning, model, restoreChatInput)
    const isFirstTurn = isResponding && messages.filter(message => message.role === 'user').length === 1
    const statusLabel = useStatusLabel(messages.at(-1))
    // The first thing the AI places is framed as if F were pressed on it, with the new groups it sits in, so a model frames whole.
    const [hasPlacedFirstDraftObject, setHasPlacedFirstDraftObject] = useState(false)
    useEffect(() => {
        if (!isFirstTurn || hasPlacedFirstDraftObject) return
        const lastMessage = messages.at(-1)
        const objectChangesPart = lastMessage?.role === 'assistant' ? lastMessage.parts.find(part => part.type === 'data-object-changes') : undefined
        const createdChanges = (objectChangesSchema.safeParse(objectChangesPart && 'data' in objectChangesPart ? objectChangesPart.data : undefined).data ?? []).filter(change => change.change === 'created')
        const firstPlaced = createdChanges.find(change => placedObjectKinds.has(change.kind))
        if (!firstPlaced || !editorState.world.objectOfId[firstPlaced.objectId]) return
        const createdObjectIds = new Set(createdChanges.map(change => change.objectId))
        const parentOfId: { [id: number]: number } = {}
        for (const [parent, children] of Object.entries(editorState.world.childrenOfId)) for (const child of children) parentOfId[child] = Number(parent)
        let objectId = firstPlaced.objectId
        while (createdObjectIds.has(parentOfId[objectId])) objectId = parentOfId[objectId]
        setHasPlacedFirstDraftObject(true)
        setFrameRequest({ objectId })
    }, [isFirstTurn, hasPlacedFirstDraftObject, messages, editorState.world])
    const [publishState, setPublishState] = useState<'idle' | 'publishing' | 'published'>('idle')
    const [isMenuCheckShown, setIsMenuCheckShown] = useState(false)
    const [gameSettings, setGameSettings] = useState({ name: gameName, description: gameDescription, suggestedMinPlayers: gameSuggestedMinPlayers, maxPlayers: gameMaxPlayers, isPublished: isGamePublished })
    const [isGameSettingsOpen, setIsGameSettingsOpen] = useState(false)
    const publish = async () => {
        setPublishState('publishing')
        try {
            // Wait for this browser's edits, including script text, before taking the server snapshot.
            await clientActionSender.waitForPendingEditAcks()
            const response = await editorRequest(inviteToken, opened.editor_url, '/publish-game', 'POST', undefined, undefined)
            const result = publishResultSchema.parse(await response.json())
            if (result.is_error) throw new Error(result.text)
            setPublishState('published')
        } catch (error) { toast.error(String(error)); setPublishState('idle') }
    }
    const setFallbackCamera = async () => {
        if (!cameraRef.current) return
        try {
            await editorRequest(inviteToken, opened.editor_url, '/set-fallback-camera', 'POST', getLocationOfEditorCamera(cameraRef.current), undefined)
            setIsMenuCheckShown(true)
            toast.success('Successfully updated thumbnail angle. Next time you publish the game, we\'ll use this camera angle.')
        } catch (error) { toast.error(String(error)) }
    }
    const refreshAiThumbnail = async () => {
        try {
            await editorRequest(inviteToken, opened.editor_url, '/refresh-ai-thumbnail', 'POST', undefined, undefined)
            setIsMenuCheckShown(true)
            toast.success('Next time you publish the game, we\'ll refresh the AI thumbnail.')
        } catch (error) { toast.error(String(error)) }
    }
    const toggleIsPublished = async () => {
        const isNowPublished = !gameSettings.isPublished
        setGameSettings(settings => ({ ...settings, isPublished: isNowPublished }))
        const refuse = (reason: string) => { setGameSettings(settings => ({ ...settings, isPublished: !isNowPublished })); toast.error(reason) }
        try {
            const result = await (isNowPublished ? publishGame : unpublishGame)(gameId, inviteToken)
            if ('error' in result) { refuse(result.error); return }
            setIsMenuCheckShown(true)
            toast.success(`Game successfully set to ${isNowPublished ? 'public' : 'private'}.`)
        } catch (error) { refuse(String(error)) }
    }
    useEffect(() => {
        if (!isMenuCheckShown) return
        const timeout = setTimeout(() => setIsMenuCheckShown(false), menuCheckShownMs)
        return () => clearTimeout(timeout)
    }, [isMenuCheckShown])
    useEffect(() => {
        if (publishState !== 'published') return
        const timeout = setTimeout(() => setPublishState('idle'), publishedLinkShownMs)
        return () => clearTimeout(timeout)
    }, [publishState])
    // Stable, so the asset cards do not render again on every edit.
    const assets = useMemo(() => {
        const selectAddedObject = (objectId: number) => {
            dispatchEditor({ type: 'select-objects', objectIds: [objectId] })
            setFrameRequest(null)
        }
        return {
            addAsset: async (assetId: number) => {
                try {
                    const response = await editorRequest(inviteToken, opened.editor_url, '/asset-download', 'POST', { asset_id: assetId }, undefined)
                    selectAddedObject(addedObjectSchema.parse(await response.json()).object_id)
                } catch (error) { toast.error(error instanceof Error ? error.message : String(error), { id: 'editor-assets' }) }
            },
            uploadAsset: async (file: File, kind: EditorAssetKind, name: string, shouldPublish: boolean) => {
                const response = await editorUploadRequest(inviteToken, opened.editor_url, `/asset-import?${new URLSearchParams({ kind, name, should_publish: String(shouldPublish) })}`, file)
                const uploaded = uploadedObjectSchema.parse(await response.json())
                selectAddedObject(uploaded.object_id)
                return uploaded.asset_id
            },
            // Each chosen image is sent as a form field named after the map it fills.
            uploadMaterial: async (name: string, textureFileOfSlot: MaterialTextureFileOfSlot, shouldPublish: boolean) => {
                const form = new FormData()
                for (const [slot, file] of Object.entries(textureFileOfSlot)) if (file !== null) form.append(slot, file)
                const response = await editorUploadRequest(inviteToken, opened.editor_url, `/asset-import-material?${new URLSearchParams({ name, should_publish: String(shouldPublish) })}`, form)
                const uploaded = uploadedObjectSchema.parse(await response.json())
                selectAddedObject(uploaded.object_id)
                return uploaded.asset_id
            },
        }
    }, [inviteToken, opened.editor_url])
    // The world as this browser shows it, script drafts included, so it downloads without the editor server.
    const downloadWorld = () => {
        const scriptDraftOfId = Object.fromEntries(Object.entries(editorState.scriptDraftOfId).filter(([, text]) => text !== undefined))
        const shownWorld = { ...editorState.world, scriptRawCodeOfId: { ...editorState.world.scriptRawCodeOfId, ...scriptDraftOfId } }
        const url = URL.createObjectURL(new Blob([JSON.stringify(shownWorld, null, 2)], { type: 'application/json' }))
        const link = document.createElement('a')
        link.href = url
        link.download = `${gameSettings.name}.kube`
        link.click()
        URL.revokeObjectURL(url)
    }
    const uploadWorld = async (uploadedWorld: string) => {
        try {
            await clientActionSender.waitForPendingEditAcks()
            const worldBeforeUpload = shownWorldRef.current
            await editorUploadRequest(inviteToken, opened.editor_url, '/upload-world', new Blob([uploadedWorld]))
            const camera = getEditorCameraOfLocation(storedWorldSchema.shape.fallback_camera_location.parse(JSON.parse(uploadedWorld).fallback_camera_location))
            if (shownWorldRef.current === worldBeforeUpload) uploadedCameraRef.current = { camera }
            else cameraRef.current = camera
        } catch (error) { toast.error(String(error)) }
    }
    const deeplinkFeedback = useDeeplinkFeedback()
    const playtest = async (operation: 'start' | 'stop' | 'add-player') => {
        setIsPlaytestPending(true)
        try {
            if (operation !== 'add-player') {
                await editorRequest(inviteToken, opened.editor_url, `/playtest/${operation}`, 'POST', undefined, undefined)
                if (operation === 'stop') return
            }
            // Starting a playtest also opens its first player through the same launch path as Spawn.
            // A guest who is logged in sends their login, so their player plays as their account instead of as "Guest".
            const loginToken = inviteToken === null ? null : (await supabaseOnClient().auth.getSession()).data.session?.access_token ?? null
            const response = await editorRequest(inviteToken, opened.editor_url, '/playtest/add-player', 'POST', { login_token: loginToken }, undefined)
            // The dev editor launches its local client directly and returns no deep link.
            if (response.status === 204) return
            const { launch_url } = z.object({ launch_url: z.string() }).parse(await response.json())
            // The app may not be installed. If the browser is still in front afterwards, offer the download.
            const browserAfterDeepLink = watchBrowserAfterDeepLink()
            openAppLink(launch_url)
            void browserAfterDeepLink.then(deeplinkFeedback.handleDeepLinkResult)
        } catch (error) { toast.error(String(error)) } finally { setIsPlaytestPending(false) }
    }
    const send = async () => {
        if (!input.trim() || isResponding || !isChatConnected) return
        const text = input.trim(); setInput('')
        await sendMessage(createEditorChatMessage(text, selectedObjects))
    }
    return <EditorLayout
        onLeave={event => { if (!confirmEditorLeave()) event.preventDefault() }}
        isCreator={isCreator}
        isGamePublic={isGamePublic}
        gameId={gameId}
        gameName={gameSettings.name}
        sidePanel={sidePanel}
        setSidePanel={setSidePanel}
        isExplorerShown={isExplorerShown}
        setIsExplorerShown={setIsExplorerShown}
        isMobilePreviewShown={isMobilePreviewShown}
        setIsMobilePreviewShown={setIsMobilePreviewShown}
        assets={assets}
        upload={assets}
        worldFile={{ isChatRunning, downloadWorld, uploadWorld }}
        inspector={<EditorObjectPanel editor={{ state: editorState, dispatch: dispatchEditor, onSelect: (objectId, isMultiSelect) => { dispatchEditor({ type: isMultiSelect ? 'toggle-select' : 'select', objectId }); setFrameRequest(null) }, onFrame: objectId => setFrameRequest({ objectId }) }} />}
        isMenuCheckShown={isMenuCheckShown}
        menuItems={<EditorMenuItems canUndo={canUndo} canRedo={canRedo} onHistoryStep={takeHistoryStep} isPlaytestRunning={isPlaytestRunning} onSetFallbackCamera={() => void setFallbackCamera()} onRefreshAiThumbnail={() => void refreshAiThumbnail()} isPublished={gameSettings.isPublished} onToggleIsPublished={() => void toggleIsPublished()} onOpenGameSettings={() => setIsGameSettingsOpen(true)} />}
        controls={<>
            {isCreator && <EditorShare gameId={gameId} isOpen={isShareOpen} setIsOpen={setIsShareOpen} />}
            {isMobile || isPlaytestRunning ? null : publishState === 'published'
                ? <Link href={GAME_PATH(gameId)} target="_blank" className={buttonClass}><Check className="size-4 text-emerald-600" />View<ExternalLink className="size-3.5" /></Link>
                : <HoverTooltip side="bottom" className="" openDelayMs={isFirstTurn ? 0 : bigButtonTooltipDelayMs} trigger={<span className="inline-flex"><button className={buttonClass} disabled={publishState === 'publishing' || isFirstTurn} onClick={() => void publish()}><CloudUpload className="size-4" />{publishState === 'publishing' ? 'Publishing…' : 'Publish'}</button></span>}>{isFirstTurn ? firstDraftButtonTooltip : 'Publish this as the latest version of the game.'}</HoverTooltip>}
            {isMobile ? null : isPlaytestRunning
                ? <>
                    <HoverTooltip side="bottom" className="" openDelayMs={bigButtonTooltipDelayMs} trigger={<span className="inline-flex"><button className={buttonClass} disabled={isPlaytestPending} onClick={() => void playtest('stop')}><Square className="size-3 fill-current" />Stop</button></span>}>Stop the test.</HoverTooltip>
                    <HoverTooltip side="bottom" className="" openDelayMs={bigButtonTooltipDelayMs} trigger={<span className="inline-flex"><button className={primaryButtonClass} disabled={isPlaytestPending} onClick={() => void playtest('add-player')}><Play className="size-4 fill-current" />Spawn</button></span>}>Launch another player on this computer.</HoverTooltip>
                </>
                : <HoverTooltip side="bottom" className="" openDelayMs={isFirstTurn ? 0 : bigButtonTooltipDelayMs} trigger={<span className="inline-flex"><button className={primaryButtonClass} disabled={isPlaytestPending || isFirstTurn} onClick={() => void playtest('start')}><Play className="size-4 fill-current" />Playtest</button></span>}>{isFirstTurn ? firstDraftButtonTooltip : 'Test locally on this computer.'}</HoverTooltip>}
            <DeeplinkFeedbackModal feedbackState={deeplinkFeedback.feedbackState} onClose={deeplinkFeedback.close} />
            <GameSettingsDialog game={{ gameId, ...gameSettings }} isOpen={isGameSettingsOpen} onOpenChange={setIsGameSettingsOpen} inviteToken={inviteToken} canDeleteGame={isCreator} onSaved={savedGameSettings => { setGameSettings(savedGameSettings); setIsMenuCheckShown(true) }} />
        </>}
        chatMessages={<>
            <EditorChatScroll>
                {/* The editor opens before the chat's saved messages arrive, so the loading placeholders stay until they do. */}
                {!isChatConnected && messages.length === 0 && <><ChatMessageSkeleton /><ChatMessageSkeleton /></>}
                <SelectedObjectKeysContext value={selectedObjectKeys}>{messages.map(message => <ChatMessage key={message.id} message={message} isReplying={isResponding && message === messages.at(-1)} onSelectObject={selectChangedObject} />)}</SelectedObjectKeysContext>
                {unsent && <div className="-mt-2 flex items-center gap-1 text-xs text-red-600">Not sent{unsent.reason && `: ${unsent.reason}`} ·<button type="button" disabled={!isChatConnected} className="underline underline-offset-2 disabled:opacity-50" onClick={() => void sendMessage(unsent.message)}>Retry</button></div>}
                {isResponding && <div className="flex gap-2 text-xs text-gray-400"><LoaderCircle className="mt-0.5 size-3 shrink-0 animate-spin" /><p><ReplyStatus message={messages.at(-1)} label={statusLabel} /></p></div>}
                {isResponding && <RunningSubagents message={messages.at(-1)} />}
            </EditorChatScroll>
        </>}
        chatComposer={<>
            {isOutOfCredits && <OutOfCreditsNotice isCreator={isCreator} onConnectYourOwnAi={() => setIsShareOpen(true)} />}
            <form autoComplete="off" data-lpignore="true" className="mx-3 mb-3 shrink-0 rounded-xl border border-gray-200 px-3 pt-2 pb-3 shadow-sm" onSubmit={event => { event.preventDefault(); void send() }}>
                <textarea aria-label="Message" autoComplete="off" data-lpignore="true" value={input} onChange={event => setInput(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send() } }} placeholder="Describe what you want to build…" rows={3} className="w-full resize-none text-[13px]/5 outline-none placeholder:text-gray-400" />
                <div className="flex items-center gap-2">
                    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                        {selectedObjects.map(selectedObject => <EditorObjectChip key={selectedObject.id} kind={getObjectKind(selectedObject)} name={selectedObject.name} onSelect={null} />)}
                    </div>
                    <EditorModelPicker model={model} onPick={pickModel} onUseYourModel={isCreator ? () => setIsShareOpen(true) : null} />
                    {isResponding ? <button type="button" aria-label="Stop reply" className={`${chatActionClass} bg-gray-950 text-white hover:bg-gray-800`} onClick={() => { void editorRequest(inviteToken, opened.chat_url, '/chat-interrupt', 'POST', undefined, undefined).catch(error => toast.error(String(error))) }}><Square className="size-3 fill-current" /></button> : <button aria-label="Send message" disabled={!input.trim() || !isChatConnected} className={`${chatActionClass} bg-gray-950 text-white disabled:opacity-30`}><ArrowUp className="size-4" /></button>}
                </div>
            </form>
        </>}
    >
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
            <EditorDocumentTabs state={editorState} dispatch={dispatchEditor} focusScriptRef={focusScriptRef} scriptEditor={<EditorScriptEditor gameId={gameId} state={editorState} dispatch={dispatchEditor} scripts={scripts} focusRef={focusScriptRef} />}>
                <EditorScene world={world} frameRequest={frameRequest} selectedObjectIds={editorState.selectedObjectIds} onSelect={(objectId, isMultiSelect) => { dispatchEditor({ type: isMultiSelect ? 'toggle-pick' : 'pick', objectId }); setFrameRequest(null) }} thumbnailUuid={thumbnailUuid} cameraRef={cameraRef} firstDraftStatus={isFirstTurn ? <ReplyStatus message={messages.at(-1)} label={hasPlacedFirstDraftObject ? 'Building' : statusLabel} /> : null} hasPlacedFirstDraftObject={hasPlacedFirstDraftObject} />
            </EditorDocumentTabs>
            {pendingHistoryStepCount > 0 && <div role="status" aria-label="Working" className="pointer-events-none absolute bottom-3 right-3 rounded-full bg-white/90 p-1.5 shadow-sm"><LoaderCircle className="size-4 animate-spin text-gray-500" /></div>}
        </div>
    </EditorLayout>
}
// Charges go to the creator, so a guest is only told why the reply stopped.
const OutOfCreditsNotice = ({ isCreator, onConnectYourOwnAi }: { isCreator: boolean, onConnectYourOwnAi: () => void }) =>
    <div role="status" className="mx-3 mb-2 shrink-0 rounded-lg border border-gray-200 px-2.5 py-1.5 text-[13px]/5 text-muted-foreground">
        {!isCreator ? <>This game&apos;s creator is out of credits.</> : <>
            Wow, you used a lot of credits! Come back tomorrow for more, or <button type="button" onClick={onConnectYourOwnAi} className="font-medium text-kube-blue hover:underline">connect your own AI</button>.
        </>}
    </div>
const placedObjectKinds = new Set(['Ball', 'Brick', 'Cylinder', 'Mesh'])
const editingToolNames = new Set(['edit_object', 'move_by_recursively', 'move_to_recursively', 'move_object_to_touch', 'rotate_recursively', 'scale_recursively', 'set_game_settings', 'catalog_item_edit'])
const getToolLabel = (toolName: string) => {
    if (toolName === 'subagent_run' || toolName === 'subagent_message') return 'Running subagents'
    if (toolName === 'sandbox_run_command') return 'Running a command'
    if (toolName === 'sandbox_view_image') return 'Looking at an image'
    if (toolName === 'asset_download_source_to_file') return 'Downloading a model'
    if (toolName.startsWith('create_') || toolName.startsWith('import_') || toolName === 'copy_and_paste' || toolName === 'asset_download') return 'Adding object'
    if (toolName === 'delete_object') return 'Deleting object'
    if (toolName === 'asset_search') return "Searching kube.new's assets"
    if (toolName.includes('playtest')) return 'Playtesting'
    if (toolName === 'write_script_raw_code' || toolName === 'edit_script_raw_code' || toolName === 'run_temporary_script') return 'Scripting'
    if (toolName === 'publish' || toolName === 'asset_publish_world_object' || toolName === 'catalog_item_publish') return 'Publishing'
    if (editingToolNames.has(toolName)) return 'Editing'
    return 'Inspecting world'
}
const parallelCallsSchema = z.object({ tool_calls: z.array(z.object({ tool_name: z.string() })) })
const getLastTool = (message: UIMessage | undefined) => message?.role === 'assistant' ? message.parts.filter(isToolUIPart).at(-1) : undefined
// One line per tool the AI is running, and one for each call inside run_parallel, until it returns.
const getStatusLabel = (message: UIMessage | undefined, staleToolCallId: string | undefined) => {
    const lastPart = message?.parts.at(-1)
    if (lastPart?.type === 'data-compaction' && !compactionSchema.safeParse(lastPart.data).success) return 'Compacting'
    const lastTool = getLastTool(message)
    if (!lastTool || lastTool.toolCallId === staleToolCallId) return 'Working'
    const toolName = getToolOrDynamicToolName(lastTool)
    if (toolName !== 'run_parallel') return getToolLabel(toolName)
    const firstCall = parallelCallsSchema.safeParse(lastTool.input).data?.tool_calls[0]
    return firstCall ? getToolLabel(firstCall.tool_name) : 'Working'
}
// TODO: AI-PICKED-VALUE: A tool's label stays this long after the tool returns, then the status goes back to Working.
const returnedToolLabelMs = 5000
const useStatusLabel = (message: UIMessage | undefined) => {
    const lastTool = getLastTool(message)
    const returnedToolCallId = lastTool?.state === 'output-available' || lastTool?.state === 'output-error' ? lastTool.toolCallId : undefined
    const [staleToolCallId, setStaleToolCallId] = useState<string>()
    useEffect(() => {
        if (returnedToolCallId === undefined) return
        const timeout = setTimeout(() => setStaleToolCallId(returnedToolCallId), returnedToolLabelMs)
        return () => clearTimeout(timeout)
    }, [returnedToolCallId])
    return getStatusLabel(message, staleToolCallId)
}
// Counts up from when the reply started, so a long one visibly keeps going. The reply carries its start time, so a reload
// or a second viewer sees the same count. The server's clock can be slightly ahead of the browser's.
const ElapsedTime = ({ startedAtMs }: { startedAtMs: number }) => {
    const [now, setNow] = useState(Date.now)
    useEffect(() => {
        const interval = setInterval(() => setNow(Date.now()), 1000)
        return () => clearInterval(interval)
    }, [])
    const seconds = Math.max(0, Math.floor((now - startedAtMs) / 1000))
    return <span className="tabular-nums">{seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`}</span>
}
const replyMetadataSchema = z.object({ startedAtMs: z.number() })
const ReplyStatus = ({ message, label }: { message: UIMessage | undefined, label: string }) => {
    const startedAtMs = message?.role === 'assistant' ? replyMetadataSchema.safeParse(message.metadata).data?.startedAtMs : undefined
    return <>{label}…{startedAtMs !== undefined && <> <ElapsedTime startedAtMs={startedAtMs} /></>}</>
}
// A subagent can work for a long time while the reply shows nothing new, so each running one is a closed Subagent N toggle
// that opens to its task, the tool it is running, how long it has worked, and the last thing it wrote. The reply has one
// progress part for each subagent, which the editor server updates.
const subagentProgressSchema = z.object({ subagentId: z.number(), task: z.string(), isRunning: z.boolean(), startedAtMs: z.number(), toolName: z.string().nullable(), latestText: z.string() })
const RunningSubagents = ({ message }: { message: UIMessage | undefined }) => {
    const subagents = message?.role === 'assistant' ? message.parts.flatMap(part => part.type === 'data-subagent' ? subagentProgressSchema.safeParse(part.data).data ?? [] : []).filter(subagent => subagent.isRunning) : []
    if (subagents.length === 0) return null
    return <div className="ml-5 space-y-2 text-xs">
        {subagents.map(subagent => <details key={subagent.subagentId} className="min-w-0">
            <summary className="w-fit cursor-pointer text-gray-500">Subagent {subagent.subagentId}</summary>
            <div className="mt-1">
                <p className="truncate text-gray-600">{subagent.task}</p>
                <p className="flex gap-2 text-gray-400">{subagent.toolName === null ? 'Working' : getToolLabel(subagent.toolName)}<ElapsedTime startedAtMs={subagent.startedAtMs} /></p>
                {subagent.latestText !== '' && <p className="line-clamp-2 text-gray-400">{subagent.latestText}</p>}
            </div>
        </details>)}
    </div>
}
const replyFailedSchema = z.object({ reason: z.string() })
const compactionSchema = z.object({ summary: z.string() })
const objectChangesSchema = z.array(z.object({ objectId: z.number(), name: z.string(), kind: z.string(), change: z.enum(['created', 'modified', 'deleted']) }))
const badgeOfChange = {
    created: { label: 'Created', className: 'bg-emerald-50 text-emerald-700' },
    modified: { label: 'Modified', className: 'bg-amber-50 text-amber-700' },
    deleted: { label: 'Deleted', className: 'bg-red-50 text-red-700' },
}
// A turn lists the objects it has changed as it goes, which is exactly what undoing it would take back.
const markdownComponents = {
    ol: ({ children, start }: { children?: ReactNode, start?: number }) => <ol start={start} className="list-outside list-decimal space-y-0.5 pl-4">{children}</ol>,
    ul: ({ children }: { children?: ReactNode }) => <ul className="list-outside list-disc space-y-0.5 pl-4">{children}</ul>,
    li: ({ children }: { children?: ReactNode }) => <li>{children}</li>,
    h1: ({ children }: { children?: ReactNode }) => <h1 className="mt-3 mb-1 text-[15px]/5 font-semibold">{children}</h1>,
    h2: ({ children }: { children?: ReactNode }) => <h2 className="mt-3 mb-1 text-sm font-semibold">{children}</h2>,
    h3: ({ children }: { children?: ReactNode }) => <h3 className="mt-3 mb-1 text-[13px]/5 font-semibold">{children}</h3>,
    h4: ({ children }: { children?: ReactNode }) => <h4 className="mt-3 mb-1 text-[13px]/5 font-semibold">{children}</h4>,
    h5: ({ children }: { children?: ReactNode }) => <h5 className="mt-3 mb-1 text-[13px]/5 font-semibold">{children}</h5>,
    h6: ({ children }: { children?: ReactNode }) => <h6 className="mt-3 mb-1 text-[13px]/5 font-semibold">{children}</h6>,
}
const selectedObjectsSchema = z.array(z.object({ objectId: z.number(), name: z.string(), kind: z.string() }))
type ChatObject = z.infer<typeof selectedObjectsSchema>[number]
const EditorObjectChip = ({ kind, name, onSelect }: { kind: string, name: string, onSelect: (() => void) | null }) => {
    const className = 'inline-flex max-w-full select-none items-center gap-1 rounded-md border border-gray-200 bg-white px-1.5 py-0.5 text-[11px]/4 text-gray-500 enabled:hover:bg-gray-50'
    const content = <>{Object.hasOwn(objectInfoSchemaOfKind, kind) && <EditorObjectIcon kind={kind} className="size-3" />}<span className="truncate">{name}</span></>
    return onSelect ? <button type="button" className={className} onClick={onSelect}>{content}</button> : <span className={`${className} cursor-default`}>{content}</span>
}
// The selection reaches only the change lists, so selecting something does not render every message again.
const SelectedObjectKeysContext = createContext('[]')
// TODO: AI-PICKED-VALUE: A turn lists at most this many changes. One that deleted thousands of keyframes drew a row for each and froze the page.
const maxShownObjectChanges = 100
const ObjectChangeList = ({ changes, onSelectObject }: { changes: z.infer<typeof objectChangesSchema>, onSelectObject: (object: ChatObject) => void }) => {
    const selectedObjects: [number, string, string][] = JSON.parse(useContext(SelectedObjectKeysContext))
    const isSelected = (object: ChatObject) => selectedObjects.some(([id, name, kind]) => id === object.objectId && name === object.name && kind === object.kind)
    return <div className="max-h-48 overflow-y-auto rounded-md border border-gray-200 py-0.5 text-[11px]/4">
        {changes.slice(0, maxShownObjectChanges).map(change => <button key={change.objectId} type="button" disabled={change.change === 'deleted'} onClick={() => onSelectObject(change)} className={`flex w-full items-center gap-1.5 px-2 py-px text-left ${isSelected(change) ? 'bg-blue-100 text-blue-700' : 'text-gray-700 enabled:hover:bg-gray-50'}`}>
            {Object.hasOwn(objectInfoSchemaOfKind, change.kind) && <EditorObjectIcon kind={change.kind} className="size-3" />}
            <span className="min-w-0 flex-1 truncate">{change.name}</span>
            <span className={`shrink-0 rounded-full px-1.5 text-[10px] leading-4 font-medium ${badgeOfChange[change.change].className}`}>{badgeOfChange[change.change].label}</span>
        </button>)}
        {changes.length > maxShownObjectChanges && <p className="px-2 py-px text-gray-400">and {(changes.length - maxShownObjectChanges).toLocaleString()} more</p>}
    </div>
}
type ChatPart = UIMessage['parts'][number]
// Keep signatures in message history, but only show reasoning that has readable text.
const isShownPart = (part: ChatPart) => part.type === 'data-reply-failed' || (part.type === 'data-compaction' && compactionSchema.safeParse(part.data).success) || ((part.type === 'text' || part.type === 'reasoning') && part.text.trim() !== '')
// Each run of reasoning goes in one Reasoning toggle. Parts the chat does not show, like tool calls, do not end a run.
const groupReasoningParts = (parts: ChatPart[]) => {
    const blocks: (ChatPart | ReasoningUIPart[])[] = []
    for (const part of parts.filter(isShownPart)) {
        const lastBlock = blocks.at(-1)
        if (part.type === 'reasoning' && Array.isArray(lastBlock)) lastBlock.push(part)
        else blocks.push(part.type === 'reasoning' ? [part] : part)
    }
    return blocks
}
// Streamdown handles incomplete Markdown while text is still arriving.
const ChatMarkdown = ({ part }: { part: TextUIPart | ReasoningUIPart }) => <Streamdown controls={false} isAnimating={part.state === 'streaming'} rehypePlugins={chatMarkdownRehypePlugins} className="space-y-2 break-words [&_pre]:text-xs [&>*:first-child]:mt-0 [&>*:last-child]:mb-0 [&_[data-code-block-container]]:my-2 [&_[data-streamdown=table-wrapper]]:my-2" components={markdownComponents}>{part.text}</Streamdown>
// The reasoning a reply is on starts open, and closes once the reply moves past it or ends. Only an open one renders its text.
const ChatReasoning = ({ parts, isRunning }: { parts: ReasoningUIPart[], isRunning: boolean }) => {
    const [isOpen, setIsOpen] = useState(isRunning)
    const [wasRunning, setWasRunning] = useState(isRunning)
    if (isRunning !== wasRunning) {
        setWasRunning(isRunning)
        setIsOpen(isRunning)
    }
    return <details open={isOpen} className="text-xs text-gray-500">
        <summary className="w-fit cursor-pointer" onClick={event => { event.preventDefault(); setIsOpen(!isOpen) }}>Reasoning</summary>
        {isOpen && <ChatReasoningText parts={parts} />}
    </details>
}
// Opening it shows the end, and it follows new text only while scrolled to the bottom.
const ChatReasoningText = ({ parts }: { parts: ReasoningUIPart[] }) => {
    const scrollRef = useRef<HTMLDivElement>(null)
    const contentRef = useRef<HTMLDivElement>(null)
    useLayoutEffect(() => {
        const scroll = scrollRef.current, content = contentRef.current
        if (!scroll || !content) return
        let previousScrollHeight: number | undefined
        const observer = new ResizeObserver(() => {
            if (previousScrollHeight === undefined || scroll.scrollTop + scroll.clientHeight >= previousScrollHeight - 1) scroll.scrollTop = scroll.scrollHeight
            previousScrollHeight = scroll.scrollHeight
        })
        observer.observe(content)
        return () => observer.disconnect()
    }, [])
    return <div ref={scrollRef} className="scroll-fade mt-2 max-h-48 overflow-y-auto">
        <div ref={contentRef} className="space-y-2">{parts.map((part, index) => <ChatMarkdown key={index} part={part} />)}</div>
    </div>
}
const ChatMessage = memo(({ message, isReplying, onSelectObject }: { message: UIMessage, isReplying: boolean, onSelectObject: (object: ChatObject) => void }) => {
    const objectChangesPart = message.parts.find(part => part.type === 'data-object-changes')
    const objectChanges = objectChangesSchema.safeParse(objectChangesPart && 'data' in objectChangesPart ? objectChangesPart.data : undefined)
    const attachedObjectsPart = message.parts.find(part => part.type === 'data-selected-objects')
    const attachedObjects = selectedObjectsSchema.safeParse(attachedObjectsPart && 'data' in attachedObjectsPart ? attachedObjectsPart.data : undefined).data ?? []
    if (!objectChanges.success && attachedObjects.length === 0 && !message.parts.some(isShownPart)) return null
    const blocks = groupReasoningParts(message.parts)
    return <div className={`text-[13px]/5 ${message.role === 'user' ? 'rounded-lg bg-gray-100 px-2.5 py-1.5' : 'flex flex-col gap-2'}`}>
        {blocks.map((part, index) => {
            if (Array.isArray(part)) return <ChatReasoning key={index} parts={part} isRunning={isReplying && index === blocks.length - 1} />
            if (part.type === 'text') return <ChatMarkdown key={index} part={part} />
            if (part.type === 'data-reply-failed') return <p key={index} className="text-xs text-red-600">Reply failed: {replyFailedSchema.safeParse(part.data).data?.reason ?? 'unknown error'}</p>
            if (part.type === 'data-compaction') return <p key={index} className="text-[11px] text-gray-400">Compacted</p>
            return null
        })}
        {attachedObjects.length > 0 && <div className="mt-1.5 flex flex-wrap gap-1">
            {attachedObjects.map(object => <EditorObjectChip key={object.objectId} kind={object.kind} name={object.name} onSelect={() => onSelectObject(object)} />)}
        </div>}
        {objectChanges.success && <ObjectChangeList changes={objectChanges.data} onSelectObject={onSelectObject} />}
    </div>
})
ChatMessage.displayName = 'ChatMessage'
