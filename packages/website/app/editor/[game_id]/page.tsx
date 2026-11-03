import type { Metadata } from 'next'
import { notFound, redirect } from 'next/navigation'
import { getLoginUrl } from '@/app/deeplink/deeplinkFlowConsts'
import { getEditorAccess } from '@/lib/editorAccess'
import { EDITOR_PATH } from '@/lib/siteNavigation'
import { getUser } from '@/lib/siteUser'
import { BrowserEditor } from './BrowserEditor'

export const metadata: Metadata = {
    title: 'Editor',
    robots: { index: false, follow: false },
    referrer: 'no-referrer',
}

// Opening a workspace only reads authorized game metadata; creation remains an explicit website action.
const Page = async ({ params, searchParams }: { params: Promise<{ game_id: string }>, searchParams: Promise<{ invite?: string | string[] }> }) => {
    const { game_id: gameIdString } = await params
    const gameId = Number(gameIdString)
    if (!Number.isSafeInteger(gameId) || gameId <= 0) notFound()

    const user = await getUser()
    const { invite } = await searchParams
    const inviteToken = typeof invite === 'string' && invite.length > 0 ? invite : null
    if (user === null && inviteToken === null) redirect(getLoginUrl({ next: EDITOR_PATH(gameId) }))

    const game = await getEditorAccess(gameId, user?.id ?? null, inviteToken)
    if (!game) notFound()

    return <BrowserEditor key={gameId} gameId={gameId} gameName={game.name} gameDescription={game.description} gameSuggestedMinPlayers={game.suggestedMinPlayers} gameMaxPlayers={game.maxPlayers} isCreator={game.isCreator} isGamePublished={game.isPublished} isGamePublic={game.isPublic} inviteToken={game.isCreator ? null : inviteToken} thumbnailUuid={game.thumbnailUuid} />
}

export default Page
