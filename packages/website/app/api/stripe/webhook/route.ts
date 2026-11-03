import type Stripe from 'stripe'
import { getStripeId, stripe, syncStripeCustomer } from '@/lib/stripe'

export const runtime = 'nodejs'

// Stripe's events for subscriptions and paid invoices. Each one only says which customer changed:
// the sync re-reads everything from Stripe, so order and repeats don't matter. Anything else, and any
// customer that isn't ours, gets a 200 so Stripe stops sending it. scripts/setup_stripe.mjs and
// scripts/run_stripe_website.sh subscribe to exactly these four.
export async function POST(request: Request) {
    const webhookSecret = process.env.STRIPE_WEBHOOK_SECRET
    if (!webhookSecret) {
        throw new Error('Missing STRIPE_WEBHOOK_SECRET in the website runtime environment')
    }
    const signature = request.headers.get('stripe-signature')
    if (signature === null) return Response.json({ error: 'Missing stripe-signature header' }, { status: 400 })

    let event: Stripe.Event
    try {
        event = stripe.webhooks.constructEvent(await request.text(), signature, webhookSecret)
    } catch {
        return Response.json({ error: 'Invalid signature' }, { status: 400 })
    }

    if (event.type === 'customer.subscription.created' || event.type === 'customer.subscription.updated' || event.type === 'customer.subscription.deleted' || event.type === 'invoice.paid') {
        const { customer } = event.data.object
        if (customer !== null) await syncStripeCustomer({ customerId: getStripeId(customer) })
    }
    return Response.json({ success: true })
}
