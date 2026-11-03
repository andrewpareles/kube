'use client'

import type { Dispatch, ReactNode, RefObject } from 'react'
import { X } from 'lucide-react'
import { Button } from '@/shadcn/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shadcn/components/ui/tabs'
import type { EditorWorldAction, EditorWorldState } from '../editorWorld'
import { EditorObjectIcon } from './EditorObjectIcon'

// Keep the scene and code editor mounted while switching documents so their camera and cursor survive.
// TODO: AI-PICKED-VALUE: Cap script names at 13rem so long names leave room for neighboring tabs.
export const EditorDocumentTabs = ({ state, dispatch, scriptEditor, focusScriptRef, children }: { state: EditorWorldState, dispatch: Dispatch<EditorWorldAction>, scriptEditor: ReactNode, focusScriptRef: RefObject<(() => void) | null>, children: ReactNode }) => <Tabs value={state.activeScriptId === null ? 'world' : String(state.activeScriptId)} onValueChange={value => dispatch(value === 'world' ? { type: 'show-world' } : { type: 'select', objectId: Number(value) })} className="h-full min-h-0 min-w-0 flex-1 gap-0">
    {state.openScriptIds.length > 0 && <TabsList aria-label="Editor documents" className="h-9 w-full shrink-0 justify-start overflow-x-auto rounded-none border-b border-gray-200 bg-white p-0">
        <TabsTrigger value="world" className="h-full flex-none rounded-none border-0 border-b-2 border-transparent px-3 text-xs data-[state=active]:border-blue-500 data-[state=active]:shadow-none">World</TabsTrigger>
        {state.openScriptIds.map(id => <div key={id} className={`flex h-full shrink-0 items-center border-b-2 ${state.activeScriptId === id ? 'border-blue-500' : 'border-transparent'}`}>
            <TabsTrigger value={String(id)} onClick={() => focusScriptRef.current?.()} className="h-full max-w-52 flex-none rounded-none border-0 pl-3 pr-1 text-xs data-[state=active]:shadow-none"><EditorObjectIcon kind="Script" className="size-3.5" /><span className="truncate">{state.world.objectOfId[id].name}</span></TabsTrigger>
            <Button type="button" variant="ghost" size="icon" aria-label={`Close ${state.world.objectOfId[id].name}`} className="mr-1 size-6 rounded-sm text-gray-400 hover:text-gray-800 [&_svg]:size-3" onClick={() => dispatch({ type: 'close-script', objectId: id })}><X /></Button>
        </div>)}
    </TabsList>}
    <TabsContent value="world" forceMount className="relative flex min-h-0 min-w-0 overflow-hidden data-[state=inactive]:hidden">{children}</TabsContent>
    {state.openScriptIds.length > 0 && <TabsContent value={state.activeScriptId === null ? 'script' : String(state.activeScriptId)} forceMount className="relative min-h-0 min-w-0 overflow-hidden data-[state=inactive]:hidden">{scriptEditor}</TabsContent>}
</Tabs>
