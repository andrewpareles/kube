'use client'

import { useEffect, useEffectEvent, useState } from 'react'
import { Bot, Check, Copy, LoaderCircle, Lock, Users } from 'lucide-react'
import { FaPeopleGroup } from 'react-icons/fa6'
import { toast } from 'sonner'
import { updateEditorShare } from '@/app/server-actions/editor-share'
import { EDITOR_PATH } from '@/lib/siteNavigation'
import { HoverTooltip } from '@/components/site/HoverTooltip'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/shadcn/components/ui/dialog'

const buttonClass = 'inline-flex items-center justify-center gap-2 rounded-lg border border-gray-200 px-3 py-2 text-sm hover:bg-gray-50 disabled:cursor-default disabled:opacity-50'

// A link on this site that carries the game's invite.
const getInviteLink = (path: string, inviteToken: string) => {
    const url = new URL(path, window.location.origin)
    url.searchParams.set('invite', inviteToken)
    return url.href
}

const CopyableLink = ({ link, label }: { link: string, label: string }) => {
    const [isCopied, setIsCopied] = useState(false)
    // TODO: AI-PICKED-VALUE: Show the checkmark for 2 seconds after a copy.
    useEffect(() => {
        if (!isCopied) return
        const timeout = setTimeout(() => setIsCopied(false), 2000)
        return () => clearTimeout(timeout)
    }, [isCopied])
    const copyLink = async () => {
        try { await navigator.clipboard.writeText(link); setIsCopied(true) }
        catch (error) { toast.error(String(error)) }
    }
    return <button onClick={() => void copyLink()} aria-label={isCopied ? `Copied ${label}` : `Copy ${label}`} className="flex min-w-0 items-center gap-3 rounded-lg border border-gray-200 bg-gray-50 px-3 py-3 text-left text-sm hover:bg-gray-100">
        <span className="min-w-0 flex-1 truncate">{link}</span>{isCopied ? <Check className="size-4 shrink-0" /> : <Copy className="size-4 shrink-0" />}
    </button>
}

// Only the creator sees these controls. Opening Share creates a token only if sharing is currently off.
// The same invite gives people the editor and gives an AI the editor's tools over MCP.
export const EditorShare = ({ gameId, isOpen, setIsOpen }: { gameId: number, isOpen: boolean, setIsOpen: (isOpen: boolean) => void }) => {
    const [isPending, setIsPending] = useState(false)
    const [inviteToken, setInviteToken] = useState<string | null>(null)

    const updateShare = async (action: 'create' | 'hide') => {
        setIsPending(true)
        try {
            const result = await updateEditorShare(gameId, action)
            if ('error' in result) { toast.error(result.error); return }
            setInviteToken(result.inviteToken)
            if (result.inviteToken === null) setIsOpen(false)
        } catch { toast.error('Could not update sharing. Try again.') } finally { setIsPending(false) }
    }

    const createLink = useEffectEvent(() => {
        setInviteToken(null)
        void updateShare('create')
    })
    useEffect(() => { if (isOpen) createLink() }, [isOpen])

    return <Dialog open={isOpen} onOpenChange={setIsOpen}>
        <HoverTooltip side="bottom" className="" trigger={<span className="inline-flex"><DialogTrigger asChild><button aria-label="Share editor" disabled={isPending} className="rounded-lg p-2 text-gray-500 hover:bg-gray-100 disabled:opacity-50"><FaPeopleGroup className="size-4" /></button></DialogTrigger></span>}>Edit with others</HoverTooltip>
        <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-md">
            <DialogHeader>
                <DialogTitle>Edit with Others</DialogTitle>
                <DialogDescription>Invite others to edit with you live in this editor.</DialogDescription>
            </DialogHeader>
            {isPending ? <div role="status" className="flex items-center gap-2 text-sm text-gray-500"><LoaderCircle className="size-4 animate-spin" />Fetching…</div> : inviteToken ? <>
                <div className="flex flex-col gap-1.5">
                    <Users aria-hidden={true} className="size-4 text-gray-500" />
                    <p className="text-xs text-gray-500">Give this to your friends.</p>
                    <CopyableLink link={getInviteLink(EDITOR_PATH(gameId), inviteToken)} label="invite link" />
                </div>
                <div className="flex flex-col gap-1.5">
                    <Bot aria-hidden={true} className="size-4 text-gray-500" />
                    <p className="text-xs text-gray-500">Share this with Codex, ChatGPT, Claude Code, Grok Bot, etc.</p>
                    <CopyableLink link={getInviteLink(`/api/ai-mcp/${gameId}`, inviteToken)} label="MCP link" />
                </div>
                <div className="mt-3 flex justify-end gap-2">
                    <HoverTooltip side="bottom" className="" trigger={<button onClick={() => void updateShare('hide')} className={buttonClass}><Lock className="size-4" />Revoke code</button>}>Turn off sharing and revoke this invite code.</HoverTooltip>
                </div>
            </> : <button onClick={() => void updateShare('create')} className={buttonClass}>Try again</button>}
        </DialogContent>
    </Dialog>
}
