import { AuthError } from "@supabase/supabase-js"
import { passwordMinLength } from "@/lib/sitePasswords"

/**
 * Our own words for each way signing in, signing up, or resetting a password can fail.
 *
 * Supabase writes its own sentence for every one of these, but they are terse and unpunctuated
 * ("Invalid login credentials"), and there is no setting anywhere that changes them. What it does
 * give us is a short code on each error that stays the same between releases, so the sentence a
 * person reads is ours and only the code comes from Supabase.
 *
 * Only the codes the login page can actually reach are here. Signing in cannot say which half was
 * wrong, the email or the password, because Supabase deliberately does not tell us: saying so would
 * let anyone check whether an account exists.
 */
const messageOfAuthErrorCode: { [code: string]: string } = {
    // Signing in.
    invalid_credentials: "That email and password don't match an account.",
    email_not_confirmed: "Check your email for the confirmation link we sent.",
    user_banned: "That account is locked. Get in touch if that's a surprise.",

    // Signing up.
    user_already_exists: "That email already exists.",
    email_exists: "That email already exists.",
    email_address_invalid: "Invalid email address.",
    signup_disabled: "Sorry, we've paused creating new accounts for now.",

    // Choosing a password, either while signing up or after following a reset link.
    weak_password: `Your password needs at least ${passwordMinLength} characters.`,
    same_password: "Your new password has to be different from your old one.",

    // Following an emailed link, which is the one thing here that goes stale on its own.
    otp_expired: "That link has expired. Ask for a new one.",
    session_not_found: "That link has expired. Ask for a new one.",
    session_expired: "That link has expired. Ask for a new one.",

    // Doing any of the above too many times in a row.
    over_email_send_rate_limit: "Too many emails just went out. Try again in a minute.",
    over_request_rate_limit: "Too many tries. Wait a minute and try again.",
}

/**
 * The sentence to show for an error Supabase handed back. An error we have no words for is written
 * to the console with its code and Supabase's own sentence, since the console is the only place its
 * cause is left after this.
 */
export const getAuthErrorMessage = ({ error }: { error: AuthError }) => {
    const message = error.code === undefined ? undefined : messageOfAuthErrorCode[error.code]
    if (message === undefined) {
        console.error(`Auth error we have no words for (${error.code}): ${error.message}`)
        return "Something went wrong. Please try again."
    }
    return message
}
