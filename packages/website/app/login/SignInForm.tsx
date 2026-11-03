'use client'

import { FormEvent, useRef } from 'react'
import { useEmailLoginState } from '@/app/login/emailFormState'

export const SignInForm = ({ next }: { next: string }) => {
    const formRef = useRef<HTMLFormElement>(null)
    const { loginState, login } = useEmailLoginState(formRef)

    const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault()
        await login({ next })
    }

    const isPending = loginState.type === 'pending'
    const error = loginState.type === 'idle' && loginState.result?.type === 'error'
        ? loginState.result.error
        : null

    return (
        <>
            <form ref={formRef} onSubmit={onSubmit} className="space-y-4">
                <div>
                    <label htmlFor="signin-email" className="block text-sm font-medium text-gray-700 mb-1">
                        Email
                    </label>
                    <input
                        id="signin-email"
                        name="email"
                        type="email"
                        required
                        title=""
                        placeholder="you@example.com"
                        disabled={isPending}
                        className="w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"
                    />
                </div>

                <div>
                    <label htmlFor="signin-password" className="block text-sm font-medium text-gray-700 mb-1">
                        Password
                    </label>
                    <input
                        id="signin-password"
                        name="password"
                        type="password"
                        required
                        minLength={6}
                        title=""
                        placeholder="Min 6 characters"
                        disabled={isPending}
                        className="w-full px-4 py-2.5 bg-white border border-border rounded-[9px] text-gray-950 placeholder:text-gray-400 focus:outline-none focus:border-kube-blue transition-colors disabled:opacity-50"
                    />
                </div>

                {error && (
                    <p className="text-sm text-red-500">{error}</p>
                )}

                <button
                    type="submit"
                    disabled={isPending}
                    className="mt-6 w-full py-2.5 px-4 bg-primary text-white font-medium rounded-xl hover:bg-primary/90 transition-colors"
                >
                    {isPending ? 'Logging in...' : 'Log In'}
                </button>
            </form>
        </>
    )
}
