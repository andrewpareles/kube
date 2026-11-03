import { getRequiredEnvVar, type EdgegapWebhookLifecycleEvent } from '@/lib/edgegapApi'
import { findEdgegapWebhookRequestId } from '@/lib/edgegapWebhookPayload'
import { deleteEditorServer, reconcileEditorServer } from '@/lib/editorServers'

// Editor lifecycle notifications use the same authentication and payload parsing as game servers.
export const handleEditorWebhookPost = async (
    request: Request,
    event: EdgegapWebhookLifecycleEvent,
) => {
    if (
        new URL(request.url).searchParams.get('secret') !==
        getRequiredEnvVar('EDGEGAP_WEBHOOK_SECRET')
    ) {
        return Response.json({ error: 'Unauthorized' }, { status: 401 })
    }
    let body: unknown
    try {
        body = await request.json()
    } catch {
        return Response.json({ error: 'Invalid JSON payload' }, { status: 400 })
    }
    const requestId = findEdgegapWebhookRequestId(body)
    if (!requestId) return Response.json({ error: 'Missing request_id' }, { status: 400 })
    console.log(`Edgegap editor webhook ${event} for ${requestId}`)
    if (event === 'ready') await reconcileEditorServer(requestId)
    else await deleteEditorServer(requestId)
    return Response.json({ success: true })
}
