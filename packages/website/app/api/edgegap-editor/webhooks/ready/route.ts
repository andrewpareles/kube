import { handleEditorWebhookPost } from '@/lib/editorServerWebhooks'

export const runtime = 'nodejs'
export const POST = async (request: Request) => handleEditorWebhookPost(request, 'ready')
