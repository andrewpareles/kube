// Where every link we send by email comes back to: confirming an account, and resetting a password. 
// It checks the token over and then sends the visitor on to wherever the mail was asked
// to take them, which for a signup confirmation is the login page.

/** NOTE: the email templates in the Supabase dashboard have to link here by hand. Both of them
carry the same three things, and only the type differs.

Confirm signup:
<p>
  <a
    href="{{ .SiteURL }}/auth/from-email-link?token_hash={{ .TokenHash }}&type=email&next={{ .RedirectTo }}"
    >Confirm your email</a>
</p>

Reset password, where the RedirectTo the reset form asks for is the page that takes the new one:
<p>
  <a
    href="{{ .SiteURL }}/auth/from-email-link?token_hash={{ .TokenHash }}&type=recovery&next={{ .RedirectTo }}"
    >Reset password</a>
</p>

the default {{ .ConfirmationURL }} is stupid.
 */


import { type EmailOtpType } from '@supabase/supabase-js'
import { type NextRequest } from 'next/server'

import { supabaseOnServer } from '@/supabase/supabaseOnServer'
import { redirect } from 'next/navigation'
import { getRedirectToUrl } from '@/app/login/loginRedirect'
import { welcomeIfNewAccount } from '@/lib/welcomeEmail'

export async function GET(request: NextRequest) {
  const { searchParams } = new URL(request.url)
  const token_hash = searchParams.get('token_hash')
  const type = searchParams.get('type') as EmailOtpType | null
  // Supabase writes the RedirectTo the signup asked for into the link as a whole url, which is
  // why this is read the same way the login page reads its own next.
  const next = getRedirectToUrl(searchParams.get('next') ?? undefined)

  if (token_hash && type) {
    const supabase = await supabaseOnServer()

    const { data, error } = await supabase.auth.verifyOtp({
      type,
      token_hash,
    })
    if (!error) {
      welcomeIfNewAccount(data.user)
      redirect(next)
    }
  }


  // redirect the user to an error page with some instructions
  redirect(`/auth/auth-code-error?error=${encodeURIComponent(`There was missing information when verifying your email. Please try again or reach out so we can help.`)}`)
}
