"use server";

import { getLoginUrl } from '@/app/deeplink/deeplinkFlowConsts'
import { getRedirectToUrl } from '@/app/login/loginRedirect'
import { planConstsOfPlanId, type PlanId } from '@/lib/pricing'
import { PLANS_RETURN_PATH, getPlansUrl } from '@/lib/siteNavigation'
import { getUser } from '@/lib/siteUser'
import { getPortalConfigurationId, getPriceIdForPlan, getStripeCustomerId, getStripeId, getStripePlanState, listPlanPrices, stripe } from '@/lib/stripe'

// Every button on the plans page comes here. What it does depends on what Stripe says the account is
// paying for right now, not on our own copy, so a sync that failed can never lead to a second
// subscription. The answer is always a URL for the browser to go to.
export const openPlan = async ({ planId, next }: { planId: PlanId, next: string }) => {
    const user = await getUser()
    if (user === null) return { url: getLoginUrl({ next: getPlansUrl({ next }) }) }

    const plansUrl = getRedirectToUrl(getPlansUrl({ next }))
    const returnUrl = getRedirectToUrl(`${PLANS_RETURN_PATH}?next=${encodeURIComponent(next)}`)
    const customerId = await getStripeCustomerId({ user })
    const planState = await getStripePlanState({ customerId })

    // Not paying for anything: Stripe's checkout. A checkout left open in another tab is closed first,
    // so paying in two tabs can't make two subscriptions.
    if (planState === undefined) {
        const [{ data: openSessions }, prices] = await Promise.all([
            stripe.checkout.sessions.list({ customer: customerId, status: 'open' }),
            listPlanPrices(),
        ])
        await Promise.all(openSessions.map((openSession) => stripe.checkout.sessions.expire(openSession.id)))
        const session = await stripe.checkout.sessions.create({
            mode: 'subscription',
            customer: customerId,
            line_items: [{ price: getPriceIdForPlan({ prices, planId }), quantity: 1 }],
            // Stripe fills in {CHECKOUT_SESSION_ID} itself, so it must not be encoded.
            success_url: `${returnUrl}&session_id={CHECKOUT_SESSION_ID}`,
            cancel_url: plansUrl,
        })
        if (session.url === null) throw new Error(`Stripe returned checkout session ${session.id} with no url`)
        return { url: session.url }
    }

    const { subscription, item, prices, schedule } = planState

    // The plan a downgrade is waiting to move to: take the downgrade back.
    if (schedule !== undefined && planId === schedule.planId) {
        await stripe.subscriptionSchedules.release(schedule.id)
        return { url: plansUrl }
    }

    // Their own plan, or an account with something to settle first: Stripe's portal, to cancel, renew,
    // pay, change card, or see invoices. An unpaid account pays there first, because a new plan's invoice
    // would become the latest and the unpaid one, once collected, would never be granted. One set to
    // cancel renews there first, because switching plans would quietly drop the cancellation. Stripe's
    // docs say the portal can't cancel a subscription with a schedule attached, so one left over from a
    // downgrade that has already happened is let go before opening it.
    if (planId === planState.planId || subscription.status === 'past_due' || subscription.cancel_at !== null) {
        if (schedule !== undefined && schedule.planId === undefined) await stripe.subscriptionSchedules.release(schedule.id)
        const portalSession = await stripe.billingPortal.sessions.create({
            customer: customerId,
            configuration: await getPortalConfigurationId({ purpose: 'manage' }),
            return_url: returnUrl,
        })
        return { url: portalSession.url }
    }

    // Switching plans replaces any downgrade that was waiting.
    if (schedule !== undefined) await stripe.subscriptionSchedules.release(schedule.id)

    // A dearer plan: Stripe's page to confirm the upgrade. Its portal configuration charges the new plan
    // in full now and starts a new month. The page shows the amount, handles a declined card or a bank
    // check, and only switches once it is paid.
    if (planConstsOfPlanId[planId].priceUsd > planConstsOfPlanId[planState.planId].priceUsd) {
        const portalSession = await stripe.billingPortal.sessions.create({
            customer: customerId,
            configuration: await getPortalConfigurationId({ purpose: 'upgrade' }),
            return_url: plansUrl,
            flow_data: {
                type: 'subscription_update_confirm',
                subscription_update_confirm: {
                    subscription: subscription.id,
                    items: [{ id: item.id, price: getPriceIdForPlan({ prices, planId }), quantity: 1 }],
                },
                after_completion: { type: 'redirect', redirect: { return_url: returnUrl } },
            },
        })
        return { url: portalSession.url }
    }

    // A cheaper plan: a subscription schedule, which is Stripe's way to change a plan at the end of the
    // billing period. They keep what they have until renewal and nothing is refunded. This follows
    // Stripe's two-step example: make a schedule from the subscription, then add the new plan as a
    // second phase starting when the current one ends.
    const createdSchedule = await stripe.subscriptionSchedules.create({ from_subscription: subscription.id })
    const [currentPhase] = createdSchedule.phases
    await stripe.subscriptionSchedules.update(createdSchedule.id, {
        // Carry on as a normal subscription after the new plan's first month. Stripe otherwise copies the
        // ending from the subscription, which would cancel it if it had been set to cancel.
        end_behavior: 'release',
        phases: [
            {
                items: [{ price: getStripeId(currentPhase.items[0].price), quantity: 1 }],
                start_date: currentPhase.start_date,
                end_date: currentPhase.end_date,
            },
            {
                items: [{ price: getPriceIdForPlan({ prices, planId }), quantity: 1 }],
                duration: { interval: 'month', interval_count: 1 },
            },
        ],
    })
    return { url: plansUrl }
}
