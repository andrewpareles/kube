import { z } from 'zod'
import type { EditorWorldState } from './editorWorld.ts'

// Compiler options arrive with the server's declarations so browser and server settings cannot drift.
export const scriptTypeEnvironmentSchema = z.object({
    declarations: z.record(z.string(), z.string()),
    compilerOptions: z.record(z.string(), z.union([z.string(), z.number(), z.boolean(), z.array(z.string()), z.record(z.string(), z.array(z.string()))])),
})
export type ScriptTypeEnvironment = z.infer<typeof scriptTypeEnvironmentSchema>
export type ScriptSnapshot = { objectId: number, path: string, text: string }
export type ScriptServices = { readTypes: (signal: AbortSignal) => Promise<ScriptTypeEnvironment> }

// Include every script, with local drafts and hierarchy paths, so diagnostics resolve relative imports.
export const getScriptSnapshots = (state: Pick<EditorWorldState, 'world' | 'scriptDraftOfId'>): ScriptSnapshot[] => {
    const scripts: ScriptSnapshot[] = []
    const visited = new Set<number>()
    const visit = (objectId: number, parentPath: string) => {
        const object = state.world.objectOfId[objectId]
        if (!object || visited.has(objectId)) return
        visited.add(objectId)
        const path = parentPath ? `${parentPath}/${object.name}` : object.name
        if (Object.hasOwn(object.info, 'Script') && Object.hasOwn(state.world.scriptRawCodeOfId, objectId)) scripts.push({ objectId, path, text: state.scriptDraftOfId[objectId] ?? state.world.scriptRawCodeOfId[objectId] })
        for (const child of state.world.childrenOfId[objectId] ?? []) visit(child, path)
    }
    visit(state.world.rootObjectId, '')
    return scripts
}

// The one stretch of the old text that differs from the new text. It never splits an emoji or other
// character made of two code units, because Monaco would widen such a range and lose half of it.
export const getChangedTextRange = (oldText: string, newText: string) => {
    let start = 0
    while (start < oldText.length && start < newText.length && oldText[start] === newText[start]) start++
    let sameEndLength = 0
    while (sameEndLength < oldText.length - start && sameEndLength < newText.length - start && oldText[oldText.length - 1 - sameEndLength] === newText[newText.length - 1 - sameEndLength]) sameEndLength++
    if (/[\uD800-\uDBFF]/.test(oldText[start - 1] ?? '')) start--
    if (/[\uDC00-\uDFFF]/.test(oldText[oldText.length - sameEndLength] ?? '')) sameEndLength--
    return { start, end: oldText.length - sameEndLength, replacement: newText.slice(start, newText.length - sameEndLength) }
}
