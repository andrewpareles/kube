import Link from 'next/link'
import type { Metadata } from 'next'
import { type ReactNode } from 'react'
import { getLoginUrl } from '@/app/deeplink/deeplinkFlowConsts'
import { getNoIndexPageMetadata } from '@/lib/siteMetadata'
import { getUser } from '@/lib/siteUser'
import { NewPasswordForm } from './NewPasswordForm'

export const metadata: Metadata = getNoIndexPageMetadata({
    title: 'Choose a New Password',
    description: null,
})

// The same card the login page sits in, on a plain background rather than the scattered games:
// somebody who has come this far is part way through something and picking a password, which is
// not a moment to be showing them what else there is to do.
const NewPasswordCard = ({ children }: { children: ReactNode }) => (
    <div className="site-backdrop-page">
        <div className="site-surface-card">
            {children}
        </div>
    </div>
)

// The end of a password reset. Following the link in the email is what signs somebody in, so
// having a session here is the proof that they read the mail, and it is what lets the password be
// changed. Anyone arriving without one has nothing to prove who they are, so all this can offer
// them is the way back to asking for a fresh link.
export default async function Page() {
    const user = await getUser()

    if (user === null) {
        return (
            <NewPasswordCard>
                <h1 className="site-page-title text-center mb-2">Reset Link Expired</h1>
                <p className="text-gray-500 text-center text-sm mb-8">Ask for a new one and it will land in your inbox.</p>
                <Link
                    href={getLoginUrl({ next: '/' })}
                    className="block w-full py-2.5 px-4 bg-primary text-white text-center font-medium rounded-xl hover:bg-primary/90 transition-colors"
                >
                    Back to Login
                </Link>
            </NewPasswordCard>
        )
    }

    return (
        <NewPasswordCard>
            <h1 className="site-page-title text-center mb-2">Choose a New Password</h1>
            <p className="text-gray-500 text-center text-sm mb-8">You are signed in as {user.email}.</p>
            <NewPasswordForm />
        </NewPasswordCard>
    )
}
