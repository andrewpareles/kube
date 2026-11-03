import { z } from 'zod'
import { storedWorldSchema, type EditorValue } from './editorObjectSchema.ts'

// This is the editor_server state shared with the browser. Rust owns the same contract
// in game_types/editor_sync.rs; drafts, selection, and AI SDK messages stay separate.
const editorStatusSchema = z.object({ is_playtest_running: z.boolean(), is_chat_running: z.boolean(), history_of_session: z.record(z.string(), z.object({ can_undo: z.boolean(), can_redo: z.boolean() })) })
export const editorStateSchema = z.object({ protocol_version: z.number(), seq: z.number(), world: storedWorldSchema, editor_status: editorStatusSchema })
export type EditorState = z.infer<typeof editorStateSchema>
export type EditorSection = 'objectOfId' | 'physicsInfo3DOfId' | 'physicsToggles3DOfId' | 'primitiveSettingsOfId' | 'scriptRawCodeOfId'
export type EditorClientDelta = { objectId: number, section: EditorSection, path: string[], value: EditorValue }
// One user action is one undo step on the editor server, however many objects it touches. The
// description is the label the inspector already showed, so the server never rebuilds it.
export type EditorClientAction = { description: string, deltas: EditorClientDelta[] }
// WASM returns the world only when it changed, alongside the batch's number and the current editor status.
export const browserEditorDeltaSchema = z.object({ seq: z.number(), world: storedWorldSchema.nullable(), editor_status: editorStatusSchema })
export const applyBrowserEditorDelta = <T extends EditorState>(state: T, delta: z.infer<typeof browserEditorDeltaSchema>): T => ({ ...state, seq: delta.seq, world: delta.world ?? state.world, editor_status: delta.editor_status })

// Live numeric edits fire on every keystroke, so each field keeps only its newest value while it waits.
// TODO: AI-PICKED-VALUE: Send at most one action every 100 ms.
export const clientSendChangeIntervalMs = 100
// Property edits and script saves share one ordered request queue. A rejected action does not stop
// the actions queued behind it. Each send answers with the number of the first world batch that
// includes that action, or the newer one that replaced it while it waited.
export const createEditorClientActionSender = (send: (action: EditorClientAction) => Promise<number>) => {
    const queued: { action: EditorClientAction, settlers: { resolve: (seq: number) => void, reject: (error: unknown) => void }[] }[] = []
    let pending: Promise<void> | null = null
    const sendPendingActions = async () => {
        let error: Error | null = null
        while (queued.length) {
            await new Promise<void>(resolve => setTimeout(resolve, clientSendChangeIntervalMs))
            const next = queued.shift()
            if (next === undefined) break
            try {
                const seq = await send(next.action)
                for (const { resolve } of next.settlers) resolve(seq)
            } catch (cause) {
                error ??= cause instanceof Error ? cause : new Error(String(cause))
                for (const { reject } of next.settlers) reject(cause)
            }
        }
        pending = null
        if (error) throw error
    }
    return {
        send: (action: EditorClientAction) => {
            let resolve!: (seq: number) => void
            let reject!: (error: unknown) => void
            const promise = new Promise<number>((resolveSeq, rejectSeq) => { resolve = resolveSeq; reject = rejectSeq })
            // A run of edits to the same fields is one undo step, so only its newest value waits to be sent.
            // Only the newest queued action can absorb it, so the server still applies edits in the order they were made.
            const isSameFields = (waiting: EditorClientAction) => waiting.deltas.length === action.deltas.length && waiting.deltas.every((waitingDelta, index) => {
                const delta = action.deltas[index]
                return waitingDelta.objectId === delta.objectId && waitingDelta.section === delta.section && waitingDelta.path.length === delta.path.length && waitingDelta.path.every((name, nameIndex) => name === delta.path[nameIndex])
            })
            const last = queued.at(-1)
            if (last === undefined || !isSameFields(last.action)) queued.push({ action, settlers: [{ resolve, reject }] })
            else {
                last.action = action
                last.settlers.push({ resolve, reject })
            }
            if (pending === null) {
                pending = sendPendingActions()
                // Each action reports its own failure; this only keeps an unwatched batch from crashing.
                pending.catch(() => {})
            }
            return promise
        },
        waitForPendingEditAcks: () => pending ?? Promise.resolve(),
    }
}
