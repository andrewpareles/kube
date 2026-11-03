'use client'

import { useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type RefObject } from 'react'
import * as monaco from 'monaco-editor/editor.js'
import 'monaco-editor/editor/browser/coreCommands.js'
import 'monaco-editor/editor/contrib/dropOrPasteInto/browser/copyPasteContribution.js'
import 'monaco-editor/editor/contrib/suggest/browser/suggestController.js'
import 'monaco-editor/features/register.all.js'
import 'monaco-editor/languages/definitions/typescript/register.js'
import { typescriptDefaults } from 'monaco-editor/languages/features/typescript/register.js'
import { toast } from 'sonner'
import type { EditorWorldAction, EditorWorldState } from '../editorWorld'
import { getChangedTextRange, getScriptSnapshots } from '../editorScripts'
import type { useEditorScripts } from './useEditorScripts'

// Load separate editor and TypeScript workers for completions, hover information, and local diagnostics.
globalThis.MonacoEnvironment = { getWorker: (_id, label) => {
    const worker = label === 'typescript' || label === 'javascript'
        ? new Worker(new URL('monaco-editor/languages/features/typescript/ts.worker.js', import.meta.url), { type: 'module' })
        : new Worker(new URL('monaco-editor/editor/editor.worker.js', import.meta.url), { type: 'module' })
    worker.addEventListener('error', event => toast.error('Script editor worker failed', { description: event.message, id: 'editor-worker-error' }))
    return worker
} }
typescriptDefaults.setEagerModelSync(true)
// TODO: AI-PICKED-VALUE: Wait 500ms after typing before checking the complete script workspace.
// Monaco provides this debounce; declarations must arrive before semantic validation starts.
typescriptDefaults.setDiagnosticsOptions({ noSemanticValidation: true, noSyntaxValidation: false, noSuggestionDiagnostics: true, onlyVisible: true })

// Text changed outside this editor, like an AI edit, becomes its own undo step. setValue would move the cursor to the top and clear undo.
const replaceModelText = (model: monaco.editor.ITextModel, text: string) => {
    const { start, end, replacement } = getChangedTextRange(model.getValue(), text)
    model.pushStackElement()
    model.pushEditOperations(null, [{ range: monaco.Range.fromPositions(model.getPositionAt(start), model.getPositionAt(end)), text: replacement }], () => null)
    model.pushStackElement()
}

// React owns script text. Monaco retains only its editing models, undo history, and view positions.
export const EditorScriptEditor = ({ gameId, state, dispatch, scripts, focusRef }: { gameId: number, state: EditorWorldState, dispatch: Dispatch<EditorWorldAction>, scripts: ReturnType<typeof useEditorScripts>, focusRef: RefObject<(() => void) | null> }) => {
    const containerRef = useRef<HTMLDivElement>(null)
    const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
    const modelStateOfIdRef = useRef<{ [id: number]: { model: monaco.editor.ITextModel, subscription: monaco.IDisposable, view: monaco.editor.ICodeEditorViewState | null } }>({})
    const extraLibOfPathRef = useRef<{ [path: string]: { content: string, subscription: monaco.IDisposable } }>({})
    const activeScriptIdRef = useRef<number | null>(null)
    const snapshots = useMemo(() => getScriptSnapshots({ world: state.world, scriptDraftOfId: state.scriptDraftOfId }), [state.world, state.scriptDraftOfId])
    const [problemCount, setProblemCount] = useState(0)
    const editScript = useEffectEvent((objectId: number, text: string) => {
        if (text !== (state.scriptDraftOfId[objectId] ?? state.world.scriptRawCodeOfId[objectId])) dispatch({ type: 'edit-script', objectId, text })
    })
    const closeScript = useEffectEvent(() => { if (state.activeScriptId !== null) dispatch({ type: 'close-script', objectId: state.activeScriptId }) })

    useEffect(() => {
        if (!scripts.typeEnvironment) return
        const { compilerOptions } = scripts.typeEnvironment
        typescriptDefaults.setCompilerOptions(compilerOptions)
        typescriptDefaults.setDiagnosticsOptions({ noSemanticValidation: false, noSyntaxValidation: false, noSuggestionDiagnostics: true, onlyVisible: true })
        return () => {
            typescriptDefaults.setDiagnosticsOptions({ noSemanticValidation: true, noSyntaxValidation: false, noSuggestionDiagnostics: true, onlyVisible: true })
        }
    }, [scripts.typeEnvironment])

    useEffect(() => {
        // Background files notify Monaco when imports change. The active model uses Monaco's typing debounce.
        const contentOfPath: { [path: string]: string } = { ...scripts.typeEnvironment?.declarations }
        for (const { objectId, path, text } of snapshots) {
            if (objectId !== state.activeScriptId) contentOfPath[monaco.Uri.from({ scheme: 'file', path: `/world/${gameId}/${path}.tsx` }).toString()] = text
        }
        const extraLibOfPath = extraLibOfPathRef.current
        for (const [path, entry] of Object.entries(extraLibOfPath)) {
            if (contentOfPath[path] === entry.content) continue
            entry.subscription.dispose(); delete extraLibOfPath[path]
            // Monaco can create models for imported files when showing related errors or definitions.
            const model = monaco.editor.getModel(monaco.Uri.parse(path))
            if (!model) continue
            if (contentOfPath[path] !== undefined) { if (model.getValue() !== contentOfPath[path]) replaceModelText(model, contentOfPath[path]) }
            else if (!Object.values(modelStateOfIdRef.current).some(entry => entry.model === model)) model.dispose()
        }
        for (const [path, content] of Object.entries(contentOfPath)) {
            if (!extraLibOfPath[path]) extraLibOfPath[path] = { content, subscription: typescriptDefaults.addExtraLib(content, path) }
        }
    }, [gameId, snapshots, state.activeScriptId, scripts.typeEnvironment])

    useEffect(() => {
        const container = containerRef.current
        if (!container) return
        // Own models created by the language service as well as the tabs, so neither outlives this workspace.
        const workspaceModels = new Set<monaco.editor.ITextModel>()
        const modelCreated = monaco.editor.onDidCreateModel(model => workspaceModels.add(model))
        const modelDisposed = monaco.editor.onWillDisposeModel(model => workspaceModels.delete(model))
        // Match the desktop editor's type size, indentation, and uncluttered code view.
        const editor = monaco.editor.create(container, { model: null, automaticLayout: true, fontFamily: 'Menlo, Monaco, Consolas, monospace', fontSize: 12, fontLigatures: false, minimap: { enabled: false }, padding: { top: 16, bottom: 16 }, tabSize: 2, theme: 'vs', wordWrap: 'off', bracketPairColorization: { enabled: true }, 'semanticHighlighting.enabled': false })
        editorRef.current = editor
        // Clicking a script tab puts the cursor in the code, including the tab already open, so a
        // shortcut pressed straight afterwards belongs to the script and not to the world.
        focusRef.current = () => editorRef.current?.focus()
        // Monaco creates its own inputs, including Find/Replace fields added after the editor mounts.
        // Watch only added elements inside this editor; React controls carry these attributes directly.
        const inputSelector = 'input, textarea, select, [contenteditable="true"], [role="textbox"]'
        const disableAutofill = (element: Element) => {
            for (const field of [element, ...element.querySelectorAll(inputSelector)]) {
                if (!field.matches(inputSelector)) continue
                field.setAttribute('autocomplete', 'off')
                field.setAttribute('data-lpignore', 'true')
            }
        }
        disableAutofill(container)
        const inputObserver = new MutationObserver(changes => {
            for (const change of changes) for (const node of change.addedNodes) if (node instanceof Element) disableAutofill(node)
        })
        inputObserver.observe(container, { childList: true, subtree: true })
        // Chrome reserves Cmd/Ctrl+W in normal tabs. The chord also works without keyboard lock.
        editor.addAction({ id: 'editor.closeScript', label: 'Close Script', keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyW, monaco.KeyMod.chord(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyK, monaco.KeyCode.KeyW)], contextMenuGroupId: '1_modification', run: () => closeScript() })
        const updateProblems = () => {
            const model = editor.getModel()
            setProblemCount(model ? monaco.editor.getModelMarkers({ resource: model.uri }).filter(marker => marker.severity === monaco.MarkerSeverity.Error || marker.severity === monaco.MarkerSeverity.Warning).length : 0)
        }
        const markers = monaco.editor.onDidChangeMarkers(updateProblems)
        const modelChanged = editor.onDidChangeModel(updateProblems)
        return () => {
            inputObserver.disconnect()
            markers.dispose(); modelChanged.dispose()
            editor.dispose()
            editorRef.current = null
            focusRef.current = null
            activeScriptIdRef.current = null
            for (const entry of Object.values(modelStateOfIdRef.current)) { entry.subscription.dispose(); entry.model.dispose() }
            modelStateOfIdRef.current = {}
            for (const entry of Object.values(extraLibOfPathRef.current)) entry.subscription.dispose()
            extraLibOfPathRef.current = {}
            for (const model of workspaceModels) model.dispose()
            modelCreated.dispose(); modelDisposed.dispose()
        }
    }, [focusRef])

    useEffect(() => {
        const editor = editorRef.current
        if (!editor) return
        const modelStateOfId = modelStateOfIdRef.current
        const previousId = activeScriptIdRef.current
        if (previousId !== null && previousId !== state.activeScriptId && modelStateOfId[previousId]) modelStateOfId[previousId].view = editor.saveViewState()
        for (const [id, entry] of Object.entries(modelStateOfId)) {
            if (state.openScriptIds.includes(Number(id))) continue
            if (editor.getModel() === entry.model) editor.setModel(null)
            entry.subscription.dispose(); entry.model.dispose(); delete modelStateOfId[Number(id)]
        }
        // Use hierarchy paths for imports, while object IDs continue to own tabs and React drafts.
        for (const { objectId: id, path, text } of snapshots) {
            if (!state.openScriptIds.includes(id)) continue
            const existing = modelStateOfId[id]
            const uri = monaco.Uri.from({ scheme: 'file', path: `/world/${gameId}/${path}.tsx` })
            if (existing?.model.uri.path === uri.path) { if (existing.model.getValue() !== text) replaceModelText(existing.model, text); continue }
            const view = existing && editor.getModel() === existing.model ? editor.saveViewState() : existing?.view ?? null
            if (existing) { if (editor.getModel() === existing.model) editor.setModel(null); existing.subscription.dispose(); existing.model.dispose() }
            const model = monaco.editor.getModel(uri) ?? monaco.editor.createModel(text, 'typescript', uri)
            if (model.getValue() !== text) model.setValue(text)
            model.updateOptions({ tabSize: 2, insertSpaces: true })
            modelStateOfId[id] = { model, subscription: model.onDidChangeContent(() => editScript(id, model.getValue())), view }
        }
        activeScriptIdRef.current = state.activeScriptId
        if (state.activeScriptId === null) return
        const entry = modelStateOfId[state.activeScriptId]
        if (editor.getModel() !== entry.model) { editor.setModel(entry.model); if (entry.view) editor.restoreViewState(entry.view) }
        editor.updateOptions({ ariaLabel: `${state.world.objectOfId[state.activeScriptId].name} script code` })
        if (previousId !== state.activeScriptId) { editor.layout(); editor.focus() }
    }, [gameId, state.activeScriptId, state.openScriptIds, snapshots, state.world.objectOfId])

    return <div className="absolute inset-0 flex flex-col">
        <div ref={containerRef} className="min-h-0 flex-1" />
        <div className="flex shrink-0 items-center gap-3 border-t border-gray-200 px-3 py-1 text-[11px] text-gray-500">
            <span role="status" className="min-w-0 flex-1">{scripts.isLoadingTypes ? 'Loading script types…' : scripts.typeEnvironment ? `${problemCount} ${problemCount === 1 ? 'problem' : 'problems'}` : null}</span>
        </div>
    </div>
}
