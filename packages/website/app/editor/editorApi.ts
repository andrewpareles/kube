import { supabaseOnClient } from '@/supabase/supabaseOnClient'
import { z } from 'zod'

// Every guest shares one invitation token, so this browser names its own editing session instead.
// The editor server attributes history entries to it, which keeps one tab's undo out of another's.
// Session storage is what makes refreshing keep the undo history: it belongs to this tab and
// outlives the page, so the reloaded editor asks the editor server for the same session's steps.
// Where a browser has none, the editor still works and only loses its place in the history.
const EDITOR_SESSION_STORAGE_KEY = 'editorSessionId'
let editorSessionId: string | null = null
const readEditorSessionId = () => {
    if (editorSessionId === null) {
        editorSessionId = globalThis.sessionStorage?.getItem(EDITOR_SESSION_STORAGE_KEY) ?? crypto.randomUUID()
        globalThis.sessionStorage?.setItem(EDITOR_SESSION_STORAGE_KEY, editorSessionId)
    }
    return editorSessionId
}
// The editor server tells every browser which sessions can undo or redo by a hash of each session, so
// this browser finds its own without anyone learning the others.
export const readEditorSessionHash = async () => {
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(readEditorSessionId()))
    return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('')
}
// An invitation stays scoped to this editor. Creator requests continue using the refreshed login session.
export const editorHeaders = async (inviteToken: string | null) => {
    const session = { 'x-editor-session': readEditorSessionId() }
    if (inviteToken !== null) return { Authorization: `Invite ${inviteToken}`, ...session }
    const { data, error } = await supabaseOnClient().auth.getSession()
    if (error || !data.session) throw new Error('Please log in again.')
    return { Authorization: `Bearer ${data.session.access_token}`, ...session }
}
const readEditorResponse = async (response: Response) => {
    if (!response.ok) {
        // HTTP errors can have empty or non-JSON bodies, especially before a server has the latest routes.
        const error = z.object({ error: z.string() }).safeParse(await response.json().catch(() => null))
        throw new Error(error.success ? error.data.error : `Editor request failed (${response.status}).`, { cause: response.status })
    }
    return response
}
export const editorRequest = async (inviteToken: string | null, apiUrl: string, path: string, method: string, body: unknown, signal: AbortSignal | undefined) => readEditorResponse(await fetch(`${apiUrl}${path}`, { method, headers: { ...await editorHeaders(inviteToken), 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal, referrerPolicy: 'no-referrer' }))
// The browser sets the content type from the body, which a multipart form needs for its boundary.
export const editorUploadRequest = async (inviteToken: string | null, apiUrl: string, path: string, body: Blob | FormData) => readEditorResponse(await fetch(`${apiUrl}${path}`, { method: 'POST', headers: await editorHeaders(inviteToken), body, referrerPolicy: 'no-referrer' }))
export const isOutOfCreditsError = (error: unknown) => error instanceof Error && error.cause === 402
