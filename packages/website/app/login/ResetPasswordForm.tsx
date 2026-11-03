'use client'

import { FormEvent, useRef } from 'react'
import { Check } from 'lucide-react'
import { useResetPasswordState } from '@/app/login/emailFormState'
import { getRedirectToUrl } from './loginRedirect'

const NEW_PASSWORD_PATH = '/auth/new-password'

type ResetPasswordFormProps = {
    onSwitchMode: ({ mode }: { mode: 'signin' | 'signup' | 'reset' }) => void
}

export const ResetPasswordForm = ({ onSwitchMode }: ResetPasswordFormProps) => {
    const formRef = useRef<HTMLFormElement>(null)
    const { resetPasswordState, resetPassword } = useResetPasswordState(formRef)

    const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault()
        await resetPassword({ emailRedirectToOnSuccess: getRedirectToUrl(NEW_PASSWORD_PATH) })
    }

    const isPending = resetPasswordState.type === 'pending'
    const error = resetPasswordState.type === 'idle' && resetPasswordState.result?.type === 'error'
        ? resetPasswordState.result.error
        : null
    const shouldShowCheckEmail = resetPasswordState.type === 'idle' && resetPasswordState.result?.type === 'checkEmail'

    return (
        <>
            <form ref={formRef} onSubmit={onSubmit} className="space-y-4">
                <div>
                    <label htmlFor="reset-email" className="block text-sm font-medium text-gray-700 mb-1">
                        Email
                    </label>
                    <input
                        id="reset-email"
                        name="email"
                        type="email"
                        required
                        title=""
                        placeholder="you@example.com"
                        disabled={isPending}
                        className="w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"
                    />
                </div>

                {error && <p className="text-sm text-red-500">{error}</p>}

                {shouldShowCheckEmail && (
                    <p className="flex items-center gap-1.5 text-sm text-gray-500"><Check className="size-4" />Please check your email.</p>
                )}

                <button
                    type="submit"
                    disabled={isPending}
                    className="mt-6 w-full py-2.5 px-4 bg-primary text-white font-medium rounded-xl hover:bg-primary/90 transition-colors"
                >
                    {isPending ? 'Sending Reset Link...' : 'Send Reset Link'}
                </button>
            </form>

            <div className="mt-6 text-center text-sm text-gray-500">
                <p>
                    Remember your password?{' '}
                    <button type="button" onClick={() => onSwitchMode({ mode: 'signin' })} className="text-gray-950 font-medium underline">
                        Log In
                    </button>
                </p>
            </div>
        </>
    )
}
