import { NextResponse } from 'next/server'
import { getRedirectToUrl } from '@/app/login/loginRedirect'
import { getUser } from '@/lib/siteUser'
import { findStripeCustomerId, getStripeId, stripe, syncStripeCustomer } from '@/lib/stripe'

// Where Stripe sends the browser back to after checkout or the portal. It pulls the account's plan
// from Stripe before redirecting, instead of waiting for the webhook.
export async function GET(request: Request) {
    const { searchParams } = new URL(request.url)

    // Checkout names its customer through the session, because Stripe's search can't see a customer
    // made a moment ago. The portal only opens for a customer that is old enough to be found.
    const getCustomerId = async () => {
        const sessionId = searchParams.get('session_id')
        if (sessionId !== null) {
            const { customer } = await stripe.checkout.sessions.retrieve(sessionId)
            return customer === null ? undefined : getStripeId(customer)
        }
        const user = await getUser()
        return user === null ? undefined : findStripeCustomerId({ profileId: user.id })
    }

    const customerId = await getCustomerId()
    if (customerId !== undefined) await syncStripeCustomer({ customerId })
    return NextResponse.redirect(getRedirectToUrl(searchParams.get('next') ?? undefined))
}
