'use client'

import { memo, useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import { ChevronDown, ChevronRight, Search } from 'lucide-react'
import { Panel, PanelGroup } from 'react-resizable-panels'
import { toast } from 'sonner'
import { getFieldLabel, getObjectKind, type StoredWorld } from '../editorObjectSchema'
import { getEditorPropertyEdits, getEditorPropertySections } from '../editorProperties'
import { getExplorerRows, type EditorWorldAction, type EditorWorldState } from '../editorWorld'
import { EditorPropertyField, PropertyRow } from './EditorPropertyField'
import { EditorObjectIcon } from './EditorObjectIcon'
import { EditorResizeHandle } from './EditorPanels'

// The explorer owns only search, expansion, and focus. Selection and object values belong to the workspace.
const EditorExplorerTree = ({ world, selectedObjectIds, isExpandedOfId, setIsExpandedOfId, search, setSearch, onSelect, onFrame }: {
    world: StoredWorld, selectedObjectIds: number[], isExpandedOfId: { [id: number]: boolean }, setIsExpandedOfId: Dispatch<SetStateAction<{ [id: number]: boolean }>>, search: string, setSearch: (search: string) => void, onSelect: (objectId: number, isMultiSelect: boolean) => void, onFrame: (objectId: number) => void
}) => {
    const selectedObjectId = selectedObjectIds.at(-1) ?? null
    const treeRef = useRef<HTMLDivElement>(null)
    const rows = useMemo(() => getExplorerRows(world, isExpandedOfId, search), [world, isExpandedOfId, search])
    // Rows include their ancestors before their children, including when search filters the tree.
    const selectedDescendantIds = useMemo(() => {
        const ids = new Set<number>()
        for (const row of rows) if (row.parentId !== null && (selectedObjectIds.includes(row.parentId) || ids.has(row.parentId))) ids.add(row.objectId)
        return ids
    }, [rows, selectedObjectIds])
    const [focusedId, setFocusedId] = useState<number | null>(null)
    const tabStop = rows.find(row => row.objectId === focusedId)?.objectId ?? rows.find(row => row.objectId === selectedObjectId)?.objectId ?? rows[0]?.objectId
    const focus = (objectId: number) => { setFocusedId(objectId); treeRef.current?.querySelector<HTMLDivElement>(`[data-object-id="${objectId}"]`)?.focus() }
    const selectionToRevealRef = useRef<number | null>(null)
    // Reveal each new selection once, waiting for collapsed ancestors to open before scrolling.
    // Expanding or collapsing other rows must not pull the view away from keyboard focus.
    useEffect(() => { selectionToRevealRef.current = selectedObjectId }, [selectedObjectId])
    useEffect(() => {
        const objectId = selectionToRevealRef.current
        if (objectId === null) return
        const tree = treeRef.current
        const selected = tree?.querySelector<HTMLDivElement>(`[data-object-id="${objectId}"]`)
        if (!tree || !selected) return
        // Reveal referenced objects without moving focus out of the property form or scrolling the page.
        const viewport = tree.getBoundingClientRect(), row = selected.getBoundingClientRect()
        if (row.top < viewport.top) tree.scrollTop += row.top - viewport.top
        else if (row.bottom > viewport.bottom) tree.scrollTop += row.bottom - viewport.bottom
        selectionToRevealRef.current = null
    }, [rows, selectedObjectId])
    return <section className="flex h-full min-h-0 flex-col" aria-label="Explorer">
        <h2 className="shrink-0 px-3 py-2 text-xs font-semibold">Explorer</h2>
        <div className="px-3 pb-2">
            <div className="flex items-center gap-2 rounded bg-gray-50 px-2 focus-within:ring-1 focus-within:ring-blue-500"><Search aria-hidden="true" className="size-3.5 text-gray-400" /><input aria-label="Search by name or id" autoComplete="off" data-lpignore="true" placeholder="Search by name or id" value={search} onChange={event => setSearch(event.target.value)} className="min-w-0 flex-1 bg-transparent py-1 text-xs outline-none" /></div>
        </div>
        <div ref={treeRef} role="tree" aria-label="Objects" aria-multiselectable="true" className="min-h-0 flex-1 overflow-auto pb-2">
            {rows.map((row, index) => {
                const object = world.objectOfId[row.objectId]
                const kind = getObjectKind(object)
                const toggle = () => setIsExpandedOfId(previous => ({ ...previous, [row.objectId]: !row.isExpanded }))
                // TODO: AI-PICKED-VALUE: Use blue-100 for the selected object and blue-50 for its descendants.
                const selectionClass = selectedObjectIds.includes(row.objectId) ? 'bg-blue-100 text-blue-700' : selectedDescendantIds.has(row.objectId) ? 'bg-blue-50 text-gray-700 hover:bg-blue-100' : 'text-gray-700 hover:bg-gray-100'
                return <div key={row.objectId} role="treeitem" aria-label={`${object.name}, ${kind}, #${object.id}`} aria-level={row.depth + 1} aria-selected={selectedObjectIds.includes(row.objectId)} aria-expanded={row.hasChildren ? row.isExpanded : undefined} data-object-id={row.objectId} tabIndex={tabStop === row.objectId ? 0 : -1} style={{ paddingLeft: 8 + row.depth * 16 }} className={`flex cursor-pointer select-none items-center gap-1 py-0.5 pr-3 text-xs outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-gray-500 ${selectionClass}`} onFocus={() => setFocusedId(row.objectId)} onClick={event => { focus(row.objectId); onSelect(row.objectId, event.shiftKey || event.metaKey || event.ctrlKey) }} onDoubleClick={() => onFrame(row.objectId)} onKeyDown={event => {
                    if (!['ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter', ' '].includes(event.key)) return
                    event.preventDefault()
                    if (event.key === 'ArrowDown' && rows[index + 1]) focus(rows[index + 1].objectId)
                    if (event.key === 'ArrowUp' && rows[index - 1]) focus(rows[index - 1].objectId)
                    if (event.key === 'Home') focus(rows[0].objectId)
                    if (event.key === 'End') focus(rows[rows.length - 1].objectId)
                    if (event.key === 'Enter' || event.key === ' ') onSelect(row.objectId, event.shiftKey || event.metaKey || event.ctrlKey)
                    if (event.key === 'ArrowRight' && row.hasChildren) { if (!row.isExpanded) toggle(); else if (rows[index + 1]) focus(rows[index + 1].objectId) }
                    if (event.key === 'ArrowLeft') { if (row.hasChildren && row.isExpanded && !search.trim()) toggle(); else if (row.parentId !== null) focus(row.parentId) }
                }}>
                    {row.hasChildren ? <button type="button" tabIndex={-1} aria-label={`${row.isExpanded ? 'Collapse' : 'Expand'} ${object.name}`} disabled={search.trim() !== ''} className="flex size-4 shrink-0 items-center justify-center text-gray-400 hover:text-gray-700" onClick={event => { event.stopPropagation(); focus(row.objectId); toggle() }} onDoubleClick={event => event.stopPropagation()}>{row.isExpanded ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}</button> : <span className="size-4 shrink-0" />}
                    <EditorObjectIcon kind={kind} className="mr-1 size-3.5" /><span className="min-w-0 flex-1 truncate">{object.name}</span>
                </div>
            })}
            {rows.length === 0 && <p className="px-3 py-2 text-xs text-gray-500">{search.trim() ? 'No matching objects.' : 'No objects.'}</p>}
        </div>
    </section>
}

// The explorer shows names, kinds and the hierarchy, so a property edit elsewhere, or the server's copy of
// the same world, does not re-render every row. Its callbacks only dispatch, so a newer copy behaves the same.
const isSameIds = (previous: number[], next: number[]) => previous.length === next.length && previous.every((id, index) => id === next[index])
const isSameExplorerWorld = (previous: StoredWorld, next: StoredWorld) => previous === next || (
    previous.rootObjectId === next.rootObjectId
    && Object.keys(previous.childrenOfId).length === Object.keys(next.childrenOfId).length
    && Object.entries(next.childrenOfId).every(([id, children]) => previous.childrenOfId[Number(id)] !== undefined && isSameIds(previous.childrenOfId[Number(id)], children))
    && Object.keys(previous.objectOfId).length === Object.keys(next.objectOfId).length
    && Object.values(next.objectOfId).every(object => {
        const before = previous.objectOfId[object.id]
        return before === object || (before !== undefined && before.name === object.name && getObjectKind(before) === getObjectKind(object))
    }))
const EditorExplorer = memo(EditorExplorerTree, (previous, next) => isSameExplorerWorld(previous.world, next.world) && isSameIds(previous.selectedObjectIds, next.selectedObjectIds) && previous.isExpandedOfId === next.isExpandedOfId && previous.search === next.search)

// The inspector edits individual paths, including the physics fields stored outside ObjectInfo.
const EditorProperties = ({ state, dispatch, onSelect }: { state: EditorWorldState, dispatch: Dispatch<EditorWorldAction>, onSelect: (objectId: number) => void }) => {
    useEffect(() => { if (state.error) toast.error(state.error, { id: 'editor-property-error' }) }, [state.error])
    const { world, selectedObjectIds } = state
    const objects = selectedObjectIds.map(id => world.objectOfId[id])
    if (!objects.length) return <p className="px-3 pb-3 text-xs text-gray-500">Select an object to inspect it.</p>
    const kinds = [...new Set(objects.map(getObjectKind))]
    const sections = getEditorPropertySections(world, selectedObjectIds)
    return <div key={selectedObjectIds.join(',')} className="grid gap-1.5 px-3 pb-3">
        {objects.length > 1 && <p className="text-xs text-gray-500">{objects.length} objects selected</p>}
        <PropertyRow label="Type" enableControl={null}><div className="pt-1.5 text-[11px] leading-4 text-gray-800">{kinds.join(', ')}</div></PropertyRow>
        {objects.length === 1 && <PropertyRow label="ID" enableControl={null}><div className="pt-1.5 text-[11px] leading-4 tabular-nums text-gray-800">{objects[0].id}</div></PropertyRow>}
        {sections.map(section => <EditorPropertyField key={section.label} label={section.label} schema={section.schema} values={section.targets.map(target => target.value)} path={[]} world={world} onSelect={onSelect} onEdit={(path, update) => dispatch({ type: 'edit', description: `${path.length ? getFieldLabel(path[path.length - 1]) : section.label} on ${objects.length > 1 ? `${objects.length} objects` : objects[0].name}`, edits: getEditorPropertyEdits(section.targets, path, update) })} />)}
    </div>
}

// Match the desktop's stacked Explorer and Properties panes, with a keyboard-accessible divider.
// TODO: AI-PICKED-VALUE: Start Explorer at 40% of a 384px sidebar; stack it below the viewport on narrow screens.
export const EditorObjectPanel = ({ editor }: { editor: { state: EditorWorldState, dispatch: Dispatch<EditorWorldAction>, onSelect: (objectId: number, isMultiSelect: boolean) => void, onFrame: (objectId: number) => void } | null }) => {
    const [search, setSearch] = useState('')
    const [isExpandedOfId, setIsExpandedOfId] = useState<{ [id: number]: boolean }>({})
    const select = (objectId: number, isMultiSelect: boolean) => editor?.onSelect(objectId, isMultiSelect)
    const revealObject = (objectId: number) => {
        if (!editor) return
        const parentOfId: { [id: number]: number } = {}
        for (const [parent, children] of Object.entries(editor.state.world.childrenOfId)) for (const child of children) parentOfId[child] = Number(parent)
        const expanded = { ...isExpandedOfId }
        const visited = new Set<number>()
        let parent = parentOfId[objectId]
        while (parent !== undefined && !visited.has(parent)) { visited.add(parent); expanded[parent] = true; parent = parentOfId[parent] }
        setSearch(''); setIsExpandedOfId(expanded)
    }
    const selectReference = (objectId: number) => { revealObject(objectId); select(objectId, false) }
    const selectedObjectId = editor?.state.selectedObjectIds.at(-1) ?? null
    // A viewport selection may be hidden by collapsed folders or search. Reveal it without stealing focus.
    // Search edits themselves must not trigger this, or typing would repeatedly clear the search.
    const onSelectionChanged = useEffectEvent(() => {
        if (!editor || selectedObjectId === null || getExplorerRows(editor.state.world, isExpandedOfId, search).some(row => row.objectId === selectedObjectId)) return
        revealObject(selectedObjectId)
    })
    useEffect(() => { onSelectionChanged() }, [selectedObjectId])
    // This page has one inspector; explicit IDs and sizes keep its server and browser markup identical.
    return <aside aria-label="Object inspector" className="h-full min-h-0 w-full bg-white">
        <PanelGroup id="editor-object-inspector" direction="vertical">
            <Panel id="editor-explorer" defaultSize={40} minSize={20}>
                {editor ? <EditorExplorer world={editor.state.world} selectedObjectIds={editor.state.selectedObjectIds} isExpandedOfId={isExpandedOfId} setIsExpandedOfId={setIsExpandedOfId} search={search} setSearch={setSearch} onSelect={select} onFrame={editor.onFrame} /> : <section><h2 className="px-3 py-2 text-xs font-semibold">Explorer</h2><div className="px-3 pb-2"><input disabled aria-label="Search by name or id" autoComplete="off" data-lpignore="true" placeholder="Search by name or id" className="w-full rounded bg-gray-50 px-2 py-1 text-xs" /></div></section>}
            </Panel>
            <EditorResizeHandle id="editor-object-divider" label="Resize Explorer and Properties" />
            <Panel id="editor-properties" defaultSize={60} minSize={20}>
                <section aria-label="Properties" className="flex h-full min-h-0 flex-col">
                    <h2 className="shrink-0 px-3 py-2 text-xs font-semibold">Properties</h2>
                    <div className="min-h-0 flex-1 overflow-y-auto">{editor && <EditorProperties state={editor.state} dispatch={editor.dispatch} onSelect={selectReference} />}</div>
                </section>
            </Panel>
        </PanelGroup>
    </aside>
}
