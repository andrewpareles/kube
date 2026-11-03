'use client'

import { createContext, use, useState, type ReactNode } from 'react'
import type { CatalogCardItem } from '@/components/site/CatalogItemCard'

// What players who haven't picked a color wear in games, set in assets/scripts/character_appearance.ts.
export const GUEST_COLOR = '#408ce6'

// The color and hat picked under My Avatar and not saved yet. Each stays undefined until it is picked, and a null hat takes the hat off.
export type AvatarDraft = { color: string | undefined, hat: CatalogCardItem | null | undefined }

// On My Avatar, your avatar wears the color and hat you are trying on before you save them.
const AvatarDraftContext = createContext<{ draft: AvatarDraft, setDraft: (draft: AvatarDraft) => void } | null>(null)

export const AvatarDraftProvider = ({ children }: { children: ReactNode }) => {
    const [draft, setDraft] = useState<AvatarDraft>({ color: undefined, hat: undefined })
    return <AvatarDraftContext.Provider value={{ draft, setDraft }}>{children}</AvatarDraftContext.Provider>
}

export const useAvatarDraft = () => use(AvatarDraftContext)
