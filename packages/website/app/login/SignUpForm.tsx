'use client'

import { FormEvent, useRef } from 'react'
import { Check } from 'lucide-react'
import { useEmailSignupState } from '@/app/login/emailFormState'
import { passwordMinLength } from '@/lib/sitePasswords'
import { toast } from 'sonner'
import { getRedirectToUrl } from './loginRedirect'

const FIELD_CLASS_NAME = "w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"

export const SignUpForm = ({ next, onSwitchMode }: { next: string, onSwitchMode: ({ mode }: { mode: 'signin' | 'signup' | 'reset' }) => void }) => {
    const formRef = useRef<HTMLFormElement>(null)
    const { signupState, signup } = useEmailSignupState(formRef)

    const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault()
        const email = new FormData(event.currentTarget).get('email') as string
        const success = await signup({ emailRedirectToOnSuccess: getRedirectToUrl(next) })
        if (success) {
            toast.success(`We sent an email to ${email}.`, {
                description: 'Check your email to confirm your account.',
                duration: Infinity,
                closeButton: true,
            })
            onSwitchMode({ mode: 'signin' })
        } else {
            toast.error('Failed to sign up.')
        }
    }

    const isPending = signupState.type === 'pending'
    const error = signupState.type === 'idle' && signupState.result?.type === 'error'
        ? signupState.result.error
        : null
    const shouldShowCheckEmail = signupState.type === 'idle' && signupState.result?.type === 'checkEmail'
    // Once the account exists the form has nothing left to do: the next step is in their email, and
    // the account is already made, so a second send from these same fields could only fail. A failed
    // sign up leaves the fields open, since fixing one and trying again is the whole point.
    const isFormDisabled = isPending || shouldShowCheckEmail

    return (
        <form ref={formRef} onSubmit={onSubmit} className="space-y-4">
            <div>
                <label htmlFor="signup-email" className="block text-sm font-medium text-gray-700 mb-1">
                    Email
                </label>
                <input
                    id="signup-email"
                    name="email"
                    type="email"
                    required
                    autoComplete="off"
                    title=""
                    placeholder="you@example.com"
                    disabled={isFormDisabled}
                    className={FIELD_CLASS_NAME}
                />
            </div>

            <div>
                <label htmlFor="signup-password" className="block text-sm font-medium text-gray-700 mb-1">
                    Password
                </label>
                <input
                    id="signup-password"
                    name="password"
                    type="password"
                    required
                    autoComplete="new-password"
                    minLength={passwordMinLength}
                    title=""
                    placeholder={`Min ${passwordMinLength} characters`}
                    disabled={isFormDisabled}
                    className={FIELD_CLASS_NAME}
                />
            </div>

            {error && <p className="text-sm text-red-500">{error}</p>}

            {shouldShowCheckEmail && (
                <p className="flex items-center gap-1.5 text-sm text-gray-500"><Check className="size-4" />Please check your email to finish signing up.</p>
            )}

            <button
                type="submit"
                disabled={isFormDisabled}
                className="mt-6 w-full py-2.5 px-4 bg-primary text-white font-medium rounded-xl hover:bg-primary/90 transition-colors disabled:hover:bg-primary"
            >
                {isPending ? 'Creating Account...' : 'Create Account'}
            </button>
        </form>
    )
}
