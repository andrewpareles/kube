import { z } from 'zod'
import { getEditorAccess } from '@/lib/editorAccess'
import { getUser } from '@/lib/siteUser'
import { EditorServerError, openEditorServer } from '@/lib/editorServers'
import { openDevEditorServer } from '@/lib/devEditorServer'

export const runtime = 'nodejs'

// The browser calls this on open and retries while its editor is starting.
export const POST = async (
    request: Request,
    { params }: { params: Promise<{ gameId: string }> },
) => {
    try {
        const gameId = z.coerce
            .number()
            .int()
            .positive()
            .safe()
            .parse((await params).gameId)
        const user = await getUser()
        const authorization = request.headers.get('authorization')
        const inviteToken = authorization?.startsWith('Invite ') ? authorization.slice(7) : null
        if (!user && !inviteToken) return Response.json({ error: 'Please log in or use an invite link.' }, { status: 401 })
        const game = await getEditorAccess(gameId, user?.id ?? null, inviteToken)
        if (!game) return Response.json({ error: 'You do not have access to this editor. The invite link may have been revoked.' }, { status: 403 })
        const opened =
            process.env.NODE_ENV === 'development'
                ? await openDevEditorServer(gameId)
                : await openEditorServer(
                      gameId,
                      z
                          .ipv4()
                          .or(z.ipv6())
                          .parse(request.headers.get('x-forwarded-for')?.split(',')[0]?.trim()),
                  )
        if (opened.status === 'starting')
            return Response.json({ status: 'starting' }, { status: 202 })
        console.log(`Opened editor ${opened.request_id} for game ${gameId}`)
        return Response.json({
            status: 'running',
            editor_url: opened.editor_url,
            chat_url: opened.chat_url,
        })
    } catch (error) {
        if (error instanceof z.ZodError)
            return Response.json({ error: 'Invalid editor request.' }, { status: 400 })
        if (error instanceof EditorServerError)
            return Response.json({ error: error.message }, { status: error.status })
        console.error('Could not open editor', error)
        return Response.json({ error: 'Could not open the editor.' }, { status: 500 })
    }
}
