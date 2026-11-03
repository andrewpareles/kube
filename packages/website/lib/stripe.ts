import 'server-only'

import Stripe from 'stripe'
import type { User } from '@supabase/supabase-js'
import { eq } from 'drizzle-orm'
import { db } from '@/lib/db'
import { plansInOrder, type PlanId } from '@/lib/pricing'
import { getPlanIdForLookupKey } from '@/lib/stripePlan'
import { userSubscriptions } from '@/supabase/migrations/schema'

const stripeSecretKey = process.env.STRIPE_SECRET_KEY
if (!stripeSecretKey) {
    throw new Error('Missing STRIPE_SECRET_KEY in the website runtime environment')
}
export const stripe = new Stripe(stripeSecretKey)

// Stripe hands back either an id or the object it names, depending on what was expanded.
export const getStripeId = (value: string | { id: string }) => typeof value === 'string' ? value : value.id

// -------------------------- Customers --------------------------

// Our server creates each Stripe customer with the profile id in its metadata, and that is how every
// path here gets from Stripe back to an account. Stripe's search lags a new customer by about a
// minute, so creating is keyed on the profile: a second click inside that minute gets the same
// customer back instead of a duplicate.
export const findStripeCustomerId = async ({ profileId }: { profileId: string }) => {
    const { data: customers } = await stripe.customers.search({ query: `metadata['kube_profile_id']:'${profileId}'` })
    return customers.length === 0 ? undefined : customers[0].id
}

export const getStripeCustomerId = async ({ user }: { user: User }) => {
    const customerId = await findStripeCustomerId({ profileId: user.id })
    if (customerId !== undefined) return customerId
    const customer = await stripe.customers.create({ email: user.email, metadata: { kube_profile_id: user.id } }, { idempotencyKey: `kube_customer_${user.id}` })
    return customer.id
}

// -------------------------- Prices and portal configurations --------------------------

export const listPlanPrices = async () => (await stripe.prices.list({ lookup_keys: plansInOrder.map((plan) => plan.id), active: true })).data

export const getPriceIdForPlan = ({ prices, planId }: { prices: Stripe.Price[], planId: PlanId }) => {
    const price = prices.find((price) => price.lookup_key === planId)
    if (price === undefined) throw new Error(`Stripe has no active price with lookup key ${planId}. Run scripts/setup_stripe.mjs.`)
    return price.id
}

// 'manage' cancels, changes card and shows invoices, and cannot switch plans. 'upgrade' only ever
// opens on Stripe's confirm page for one switch to a dearer plan. scripts/setup_stripe.mjs makes both.
export const getPortalConfigurationId = async ({ purpose }: { purpose: 'manage' | 'upgrade' }) => {
    const { data: configurations } = await stripe.billingPortal.configurations.list({ active: true, limit: 100 })
    const configuration = configurations.find((configuration) => configuration.metadata?.kube_portal === purpose)
    if (configuration === undefined) throw new Error(`Stripe has no active portal configuration for ${purpose}. Run scripts/setup_stripe.mjs.`)
    return configuration.id
}

// -------------------------- What someone is paying for --------------------------

// Stripe's own words for a subscription that is being paid for. Anything else - canceled, unpaid,
// incomplete - leaves the account on the free tier, whatever plan it names.
const ACTIVE_SUBSCRIPTION_STATUSES = ['active', 'trialing', 'past_due']

// Read from Stripe as it is right now. schedule is set when a downgrade was scheduled, and its planId
// is the plan it moves to at renewal, or undefined once that move has already happened.
export const getStripePlanState = async ({ customerId }: { customerId: string }) => {
    const [{ data: subscriptions }, prices] = await Promise.all([
        stripe.subscriptions.list({ customer: customerId, expand: ['data.schedule'] }),
        listPlanPrices(),
    ])
    const subscription = subscriptions.find((subscription) => ACTIVE_SUBSCRIPTION_STATUSES.includes(subscription.status))
    if (subscription === undefined) return undefined

    const [item] = subscription.items.data
    const planId = getPlanIdForLookupKey(item.price.lookup_key)
    if (planId === undefined) throw new Error(`Subscription ${subscription.id} is on price ${item.price.id}, which is not a plan`)

    const attachedSchedule = subscription.schedule
    const scheduledPhase = attachedSchedule === null || typeof attachedSchedule === 'string'
        ? undefined
        : attachedSchedule.phases.find((phase) => phase.start_date >= item.current_period_end)
    const scheduledPriceId = scheduledPhase === undefined ? undefined : getStripeId(scheduledPhase.items[0].price)
    const schedule = attachedSchedule === null
        ? undefined
        : { id: getStripeId(attachedSchedule), planId: getPlanIdForLookupKey(prices.find((price) => price.id === scheduledPriceId)?.lookup_key) }

    return { subscription, item, prices, planId, schedule, periodEnd: item.current_period_end }
}

// The plans page asks only for accounts our own copy says are paying, so free accounts cost no call.
export const getStripePlanStateForProfile = async ({ profileId }: { profileId: string }) => {
    const [row] = await db
        .select({ stripeCustomerId: userSubscriptions.stripeCustomerId, status: userSubscriptions.status })
        .from(userSubscriptions)
        .where(eq(userSubscriptions.profileId, profileId))
    if (row === undefined || !ACTIVE_SUBSCRIPTION_STATUSES.includes(row.status)) return undefined
    // A row can name a customer this Stripe account doesn't have, such as one a local checkout made in
    // the sandbox. The account then shows as free rather than the page failing.
    try {
        return await getStripePlanState({ customerId: row.stripeCustomerId })
    } catch (error) {
        if (error instanceof Stripe.errors.StripeInvalidRequestError && error.code === 'resource_missing' && error.param === 'customer') return undefined
        throw error
    }
}

// -------------------------- Syncing an account from Stripe --------------------------

// The one path that writes a subscription, used by both the webhook and
// the page Stripe returns the browser to. It always re-reads Stripe rather than trusting what an event
// carried, so events arriving late or out of order still leave the row matching Stripe.
export const syncStripeCustomer = async ({ customerId }: { customerId: string }) => {
    const customer = await stripe.customers.retrieve(customerId)
    if (customer.deleted) {
        await db.update(userSubscriptions).set({ status: 'canceled' }).where(eq(userSubscriptions.stripeCustomerId, customerId))
        return
    }
    // A customer our server did not create, such as one left over from Autumn, belongs to no account.
    const profileId = customer.metadata.kube_profile_id
    if (profileId === undefined) return

    // The paying subscription if there is one, and otherwise the newest, so a cancellation is recorded.
    const { data: subscriptions } = await stripe.subscriptions.list({ customer: customerId, status: 'all' })
    const payingSubscription = subscriptions.find((subscription) => ACTIVE_SUBSCRIPTION_STATUSES.includes(subscription.status))
    const subscription = payingSubscription === undefined ? subscriptions[0] : payingSubscription
    if (subscription === undefined) return
    const [item] = subscription.items.data
    const plan = getPlanIdForLookupKey(item.price.lookup_key)
    if (plan === undefined) return

    const subscriptionState = { plan, status: subscription.status, stripeCustomerId: customerId, currentPeriodEnd: new Date(item.current_period_end * 1000).toISOString() }
    await db.insert(userSubscriptions).values({ profileId, ...subscriptionState }).onConflictDoUpdate({ target: userSubscriptions.profileId, set: subscriptionState })
}
