'use client'

import { useState } from 'react'
import { ArrowUpRight, Check } from 'lucide-react'
import { toast } from 'sonner'
import { openPlan } from '@/app/server-actions/plans'
import { openLoginModal } from '@/app/login/LoginModal'
import { useUser } from '@/components/context/useUser'
import type { PlanId } from '@/lib/pricing'
import { getPlansUrl } from '@/lib/siteNavigation'

// The label, and the note shown above the button, are worked out on the server from what the account
// is paying for. The click always asks the server again, which decides from Stripe what actually happens.
export const PlanButton = ({ planId, next, label, note, isArrowShown, isDisabled }: { planId: PlanId, next: string, label: string, note: string | undefined, isArrowShown: boolean, isDisabled: boolean }) => {
    const [isOpening, setIsOpening] = useState(false)
    const user = useUser()

    const onClick = async () => {
        if (user === null) {
            openLoginModal({ next: getPlansUrl({ next }) })
            return
        }
        setIsOpening(true)
        try {
            const { url } = await openPlan({ planId, next })
            window.location.assign(url)
        } catch {
            toast.error('Something went wrong. Please try again.')
            setIsOpening(false)
        }
    }

    return (
        <>
            {note !== undefined && (
                <p className="plan-button-note">
                    <Check className="plan-button-note-icon" aria-hidden="true" />
                    {note}
                </p>
            )}
            <button type="button" className="site-button plan-button" disabled={isOpening || isDisabled} onClick={() => void onClick()}>
                {label}
                {isArrowShown && <ArrowUpRight className="plan-button-icon" aria-hidden="true" />}
            </button>
        </>
    )
}
