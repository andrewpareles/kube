'use client'

import { useState, type ReactNode } from 'react'
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels'

// Every divider is one visible line; the panel library expands its pointer hit area for dragging.
export const EditorResizeHandle = ({ id, label }: { id: string, label: string }) => <PanelResizeHandle id={id} aria-label={label} className="relative w-px shrink-0 bg-gray-200 outline-none transition-colors data-[panel-group-direction=vertical]:h-px data-[panel-group-direction=vertical]:w-full data-[resize-handle-state=hover]:bg-gray-400 data-[resize-handle-state=drag]:bg-blue-500 focus-visible:bg-blue-500" />

// Explicit panel IDs and starting sizes keep server rendering and hydration consistent.
// TODO: AI-PICKED-VALUE: Start side panels at 25%, with 15% minimums and at least 20% left for the viewport.
// Adding or removing a panel lays the group out again from default sizes, so they follow the current sizes.
export const EditorPanels = ({ chat, sidePanel, inspector, children }: { chat: ReactNode, sidePanel: ReactNode, inspector: ReactNode, children: ReactNode }) => {
    const [sizes, setSizes] = useState({ chat: 25, sidePanel: 25, sidebar: 25 })
    const sidebarSize = inspector ? sizes.sidebar : 0
    const sidePanelSize = sidePanel ? Math.max(15, Math.min(sizes.sidePanel, 80 - sizes.chat - sidebarSize)) : 0
    return <PanelGroup id="editor-workspace" direction="horizontal" onLayout={layout => setSizes(previous => ({ chat: layout[0], sidePanel: sidePanel ? layout[2] : previous.sidePanel, sidebar: inspector ? layout[sidePanel ? 3 : 2] : previous.sidebar }))}>
        <Panel id="editor-chat" order={1} defaultSize={sizes.chat} minSize={15}>
            <aside aria-label="AI chat" className="flex h-full min-h-0 min-w-0 flex-col">{chat}</aside>
        </Panel>
        <EditorResizeHandle id="editor-chat-divider" label="Resize chat and viewport" />
        <Panel id="editor-viewport" order={2} defaultSize={100 - sizes.chat - sidebarSize - sidePanelSize} minSize={20} className="flex min-h-0 min-w-0 flex-col">{children}</Panel>
        {sidePanel && <>
            <EditorResizeHandle id="editor-side-panel-divider" label="Resize viewport and side panel" />
            <Panel id="editor-side-panel" order={3} defaultSize={sidePanelSize} minSize={15}>{sidePanel}</Panel>
        </>}
        {inspector && <>
            <EditorResizeHandle id="editor-inspector-divider" label="Resize viewport and inspector" />
            <Panel id="editor-sidebar" order={4} defaultSize={sizes.sidebar} minSize={15}>{inspector}</Panel>
        </>}
    </PanelGroup>
}
