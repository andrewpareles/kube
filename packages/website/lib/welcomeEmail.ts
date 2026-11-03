import 'server-only'

import type { User } from '@supabase/supabase-js'
import { eq } from 'drizzle-orm'
import { after } from 'next/server'
import { Resend } from 'resend'
import { db } from '@/lib/db'
import { getSiteUrl } from '@/lib/siteMetadata'
import { CREATE_PATH } from '@/lib/siteNavigation'
import { userProfiles } from '@/supabase/migrations/schema'

const resendApiKey = process.env.RESEND_API_KEY
if (!resendApiKey) {
    throw new Error('Missing RESEND_API_KEY in the website runtime environment')
}
const resend = new Resend(resendApiKey)

// TODO: AI-PICKED-VALUE: The sender address and the email's styling, the same as the game published email the editor server sends. updates.kube.new is the domain verified in Resend.
const WELCOME_EMAIL_FROM = 'kube.new <noreply@updates.kube.new>'
const WELCOME_EMAIL_REPLY_TO = 'andrew@kube.new'
// TODO: AI-PICKED-VALUE: An account whose email was confirmed in the last minute is new. Google accounts are confirmed when they're created, and email accounts when their link is opened.
const NEW_ACCOUNT_MAX_AGE_MS = 60_000

export const sendWelcomeEmail = async ({ email, username }: { email: string, username: string }) => {
    const guestNote = username.startsWith('Guest-') ? ' (you should change your username!)' : ''
    const { error } = await resend.emails.send({
        from: WELCOME_EMAIL_FROM,
        to: email,
        replyTo: WELCOME_EMAIL_REPLY_TO,
        subject: 'Welcome!',
        html: `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body style="margin: 0; padding: 0;">
<div style="max-width: 520px; margin: 0 auto; padding: 32px 24px; font-family: Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; font-size: 16px; line-height: 1.6;">
<p style="margin: 0 0 20px;">Hey ${username}${guestNote},</p>
<p style="margin: 0 0 20px; font-size: 22px; font-weight: 700; line-height: 1.3;">Welcome to Kube!</p>
<p style="margin: 0 0 20px;">Want to make a game without coding?</p>
<p style="margin: 0 0 20px;"><a href="${getSiteUrl(CREATE_PATH)}" style="color: #336dff; font-weight: 600;">Make the game</a> you always wanted to without coding.</p>
<p style="margin: 0 0 20px;">You can even edit live with friends. Oh, and it's totally free to start.</p>
<p style="margin: 0 0 20px;">Let me know if you have any feedback - good or bad. And welcome to the kube community! Feel free to join our <a href="https://discord.gg/4uZsGRtGT" style="color: #336dff;">Discord</a>.</p>
<p style="margin: 28px 0 0;">- Kubester</p>
</div>
</body>
</html>
`,
    })
    if (error !== null) {
        throw new Error(`Could not send the welcome email: ${error.message}`)
    }
}

// Every way of signing in calls this once it has the user. The email goes out after the response, so it never slows down
// or fails signing in.
export const welcomeIfNewAccount = (user: User | null) => {
    if (user === null || user.email === undefined || user.email_confirmed_at === undefined) return
    if (Date.now() - Date.parse(user.email_confirmed_at) > NEW_ACCOUNT_MAX_AGE_MS) return
    const email = user.email
    after(async () => {
        const [profile] = await db.select({ username: userProfiles.username }).from(userProfiles).where(eq(userProfiles.id, user.id))
        await sendWelcomeEmail({ email, username: profile.username })
    })
}
