'use client'

import { useEffect, useMemo, useState } from 'react'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'
import { GUEST_COLOR, useAvatarDraft } from '@/components/site/AvatarDraft'
import type { PlayerLook } from '@/lib/siteAvatar'

// Every avatar on the page shares one download of the default character.
let defaultCharacterPromise: Promise<unknown> | null = null
const loadDefaultCharacter = () => {
    defaultCharacterPromise ??= fetch('/api/default-character')
        .then(response => {
            if (!response.ok) throw new Error(`The default character could not be loaded (${response.status}).`)
            return response.json()
        })
        .catch(error => {
            defaultCharacterPromise = null
            throw error
        })
    return defaultCharacterPromise
}

// A player's avatar is the default character in their hat and preferred color, drawn in a circle. The one on a profile moves in its idle
// pose, can be dragged to turn it around, and wears the color and hat you are trying on in My Avatar before you save them. The small ones
// in the header, the sidebar and a game's server list stay still and sit in links, so the pointer passes through to the link.
export const PlayerAvatar = ({ look, isProfileAvatar, className }: { look: PlayerLook, isProfileAvatar: boolean, className: string }) => {
    const [character, setCharacter] = useState<unknown>(null)
    const draft = useAvatarDraft()?.draft
    const hat = draft?.hat === undefined ? look.equippedItem : draft.hat
    useEffect(() => {
        let isCurrent = true
        loadDefaultCharacter()
            .then(loaded => {
                if (isCurrent) setCharacter(loaded)
            })
            .catch(console.error)
        return () => {
            isCurrent = false
        }
    }, [])
    const assetDataJson = useMemo(
        () => character === null ? null : JSON.stringify({ Avatar: { character, hat, color: look.preferredColor ?? GUEST_COLOR, isInRestPose: !isProfileAvatar } }),
        [character, hat, look.preferredColor, isProfileAvatar],
    )
    // My Avatar's avatar is repainted in place, so trying on colors doesn't load it again.
    const avatarColor = isProfileAvatar ? draft?.color ?? look.preferredColor ?? GUEST_COLOR : null

    return (
        <span className={`block shrink-0 overflow-hidden rounded-full avatar-sky ${isProfileAvatar ? '' : 'pointer-events-none'} ${className}`}>
            {assetDataJson !== null && <AssetScenePreview assetDataJson={assetDataJson} assetName="avatar" mode="spin" controls={null} footer={null} isInspectable={isProfileAvatar} avatarColor={avatarColor} isAlwaysAnimated={isProfileAvatar} />}
        </span>
    )
}
