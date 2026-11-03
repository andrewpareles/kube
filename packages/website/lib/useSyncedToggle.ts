'use client'

import { useRef, useTransition } from 'react'
import { toast } from 'sonner'
import { useRefState } from '@/lib/useRefState'

export const useSyncedToggle = ({ isOn, saveIsOn, errorMessage }: { isOn: boolean, saveIsOn: (isOn: boolean) => Promise<boolean>, errorMessage: string }) => {
    const [isOnRef, setIsOn] = useRefState(isOn)
    const savedIsOnRef = useRef(isOn)
    const isSavingRef = useRef(false)
    const [, startTransition] = useTransition()

    const save = () => {
        if (isSavingRef.current) {
            return
        }
        isSavingRef.current = true

        startTransition(async () => {
            try {
                while (savedIsOnRef.current !== isOnRef.current) {
                    savedIsOnRef.current = await saveIsOn(isOnRef.current)
                }
                setIsOn(savedIsOnRef.current)
            } catch {
                setIsOn(savedIsOnRef.current)
                toast.error(errorMessage)
            } finally {
                isSavingRef.current = false
                if (savedIsOnRef.current !== isOnRef.current) {
                    save()
                }
            }
        })
    }

    const toggle = () => {
        setIsOn(!isOnRef.current)
        save()
    }

    return { isOn: isOnRef.current, toggle }
}
