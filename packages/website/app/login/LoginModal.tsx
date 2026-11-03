'use client'

import { useState } from 'react'
import { useSearchParams } from 'next/navigation'
import { SignInForm } from './SignInForm'
import { SignUpForm } from './SignUpForm'
import { ResetPasswordForm } from './ResetPasswordForm'
import { GoogleSignInButton } from './GoogleSignInButton'
import { Check } from 'lucide-react'
import { useUser } from '@/components/context/useUser'
import { ModalFrame } from '@/components/site/SiteModalFrame'
import { LOGIN_SEARCH_PARAM } from '@/app/deeplink/deeplinkFlowConsts'
import { CREATE_PATH } from '@/lib/siteNavigation'

type Mode = 'signin' | 'signup' | 'reset'

const MakeAnAccountPrompt = ({ onSwitchMode }: { onSwitchMode: ({ mode }: { mode: Mode }) => void }) => (
    <p className="mt-4 text-center text-sm text-gray-500">
        {"Don't have an account? "}
        <button type="button" onClick={() => onSwitchMode({ mode: 'signup' })} className="text-gray-950 font-medium underline">
            Sign Up
        </button>
    </p>
)

const AlreadyHaveAnAccountPrompt = ({ onSwitchMode }: { onSwitchMode: ({ mode }: { mode: Mode }) => void }) => (
    <p className="mt-4 text-center text-sm text-gray-500">
        {'Already have an account? '}
        <button type="button" onClick={() => onSwitchMode({ mode: 'signin' })} className="text-gray-950 font-medium underline">
            Log In
        </button>
    </p>
)

const ForgotPasswordLink = ({ onSwitchMode }: { onSwitchMode: ({ mode }: { mode: Mode }) => void }) => (
    <p className="mt-4 text-center text-sm text-gray-500">
        <button type="button" onClick={() => onSwitchMode({ mode: 'reset' })} className="hover:text-gray-950 transition-colors">
            Forgot password?
        </button>
    </p>
)

const OrDivider = () => (
    <div className="my-6 flex items-center gap-3 text-xs text-gray-400">
        <div className="h-px flex-1 bg-gray-200" />
        or
        <div className="h-px flex-1 bg-gray-200" />
    </div>
)

const SavedPromptNote = ({ className }: { className: string }) => (
    <div className={`${className} flex items-start gap-2.5 rounded-xl border border-gray-200 bg-gray-50 px-3.5 py-3 text-sm text-gray-700`}>
        <Check className="mt-px size-4 shrink-0 text-gray-500" />
        <p>Your prompt is saved. Log in to continue.</p>
    </div>
)

const LoginModalContent = ({ next }: { next: string }) => {
    const [mode, setMode] = useState<Mode>('signin')
    const switchMode = ({ mode }: { mode: Mode }) => setMode(mode)

    const title = mode === 'signin' ? 'Log In' : mode === 'signup' ? 'Create an Account' : 'Reset Your Password'
    // Browser creation saves the prompt before returning here for login.
    const shouldShowSavedPrompt = mode !== 'reset' && next === CREATE_PATH

    return (
        <div className="site-login-modal flex flex-col">
            <h1 className="site-page-title text-center mb-2">{title}</h1>
            {/* The gap under the title is small, which is right for a subtitle because it reads as
                part of the title. The note is its own block rather than a subtitle, so it takes the
                same gap above and below instead of sitting up against the title. */}
            <div className={'mb-8'}>
                {shouldShowSavedPrompt && <SavedPromptNote className="mt-4" />}
            </div>

            {mode !== 'reset' && <GoogleSignInButton next={next} />}
            {mode !== 'reset' && <OrDivider />}
            {mode === 'signin' && <SignInForm next={next} />}
            {mode === 'signup' && <SignUpForm next={next} onSwitchMode={switchMode} />}
            {mode === 'reset' && <ResetPasswordForm onSwitchMode={switchMode} />}
            {mode === 'signin' && <MakeAnAccountPrompt onSwitchMode={switchMode} />}
            {mode === 'signin' && <ForgotPasswordLink onSwitchMode={switchMode} />}
            {mode === 'signup' && <AlreadyHaveAnAccountPrompt onSwitchMode={switchMode} />}
        </div>
    )
}

// replaceState asks nothing of the server, and Next keeps useSearchParams in step with it.
export const openLoginModal = ({ next }: { next: string }) => {
    const url = new URL(window.location.href)
    url.searchParams.set(LOGIN_SEARCH_PARAM, next)
    window.history.replaceState(null, '', url)
}

const closeLoginModal = () => {
    const url = new URL(window.location.href)
    url.searchParams.delete(LOGIN_SEARCH_PARAM)
    window.history.replaceState(null, '', url)
}

// The content is its own component so it starts over at Log In each time the modal opens.
export const LoginModal = () => {
    const user = useUser()
    const next = useSearchParams().get(LOGIN_SEARCH_PARAM)
    if (user !== null || next === null) return null

    return (
        <ModalFrame isOpen={true} onClose={closeLoginModal}>
            <LoginModalContent next={next} />
        </ModalFrame>
    )
}
