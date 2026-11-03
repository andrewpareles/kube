'use client'

import { useState } from 'react'
import { Check, ChevronDown, ExternalLink } from 'lucide-react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/shadcn/components/ui/dropdown-menu'

// crates/editor_server/node/src/helpers/model.ts modelIds. The two must agree.
const editorModels = [
    { id: 'anthropic.claude-opus-5-5', label: 'Opus 5.5', isRecommended: true },
    { id: 'openai.gpt-6-astra', label: 'GPT 6 Astra', isRecommended: false },
] as const
export type EditorModelId = (typeof editorModels)[number]['id']
const modelStorageKey = 'editor-ai-model'

const readStoredModel = (): EditorModelId => {
    try {
        const stored = localStorage.getItem(modelStorageKey)
        return editorModels.find(model => model.id === stored)?.id ?? editorModels[0].id
    } catch { return editorModels[0].id }
}

export const useEditorModel = () => {
    const [model, setModel] = useState(readStoredModel)
    const pickModel = (id: EditorModelId) => {
        setModel(id)
        try { localStorage.setItem(modelStorageKey, id) } catch {}
    }
    return [model, pickModel] as const
}

export const EditorModelPicker = ({ model, onPick, onUseYourModel }: { model: EditorModelId, onPick: (model: EditorModelId) => void, onUseYourModel: (() => void) | null }) =>
    <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
            <button type="button" className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg px-2 text-xs text-gray-500 hover:bg-gray-100">{editorModels.find(item => item.id === model)?.label}<ChevronDown className="size-3" /></button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" side="top" className="w-64">
            {editorModels.map(item => <DropdownMenuItem key={item.id} onClick={() => onPick(item.id)} className="text-[13px]">
                {item.label}{item.isRecommended && <span className="text-muted-foreground">(Recommended)</span>}
                {item.id === model && <Check className="ml-auto size-4" />}
            </DropdownMenuItem>)}
            {onUseYourModel && <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={onUseYourModel} className="items-start">
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span className="text-[13px]">Your Model</span>
                        <span className="text-xs text-muted-foreground">Edit for free with your own subscription (ChatGPT, Claude, etc).</span>
                    </div>
                    <ExternalLink className="mt-0.5 size-3.5 shrink-0" />
                </DropdownMenuItem>
            </>}
        </DropdownMenuContent>
    </DropdownMenu>
