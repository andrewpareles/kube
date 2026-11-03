'use client'

import { FormEvent, useRef } from 'react'
import { useNewPasswordState } from '@/app/login/emailFormState'
import { getRedirectToUrl } from '@/app/login/loginRedirect'
import { passwordMinLength } from '@/lib/sitePasswords'

export const NewPasswordForm = () => {
    const formRef = useRef<HTMLFormElement>(null)
    const { newPasswordState, setNewPassword } = useNewPasswordState(formRef)

    const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault()
        await setNewPassword({ redirectToOnSuccess: getRedirectToUrl('/') })
    }

    const isPending = newPasswordState.type === 'pending'
    const error = newPasswordState.type === 'idle' && newPasswordState.result?.type === 'error'
        ? newPasswordState.result.error
        : null

    return (
        <form ref={formRef} onSubmit={onSubmit} className="space-y-4">
            <div>
                <label htmlFor="new-password" className="block text-sm font-medium text-gray-700 mb-1">
                    New Password
                </label>
                <input
                    id="new-password"
                    name="password"
                    type="password"
                    required
                    minLength={passwordMinLength}
                    title=""
                    autoComplete="new-password"
                    placeholder={`Min ${passwordMinLength} characters`}
                    disabled={isPending}
                    className="w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"
                />
            </div>

            {error && <p className="text-sm text-red-500">{error}</p>}

            <button
                type="submit"
                disabled={isPending}
                className="mt-6 w-full py-2.5 px-4 bg-primary text-white font-medium rounded-xl hover:bg-primary/90 transition-colors"
            >
                {isPending ? 'Saving Password...' : 'Save Password'}
            </button>
        </form>
    )
}
