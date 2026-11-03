// Creates or updates everything kube.new needs in a Stripe account and touches nothing else, so it is
// safe to run again. The plans and their prices come from packages/website/lib/pricing.ts.
//
//   Sandbox: STRIPE_SECRET_KEY=sk_test_... node scripts/setup_stripe.mjs
//   Live:    STRIPE_SECRET_KEY="$(cat ~/.kube_stripe_live_key)" node scripts/setup_stripe.mjs --webhook-url https://kube.new/api/stripe/webhook
//
// A webhook's signing secret is only shown once, when the webhook is created, so it is written to
// ~/.kube_stripe_webhook_secret rather than printed.

import { writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
import { plansInOrder } from '../packages/website/lib/pricing.ts'

const stripeSecretKey = process.env.STRIPE_SECRET_KEY
if (!stripeSecretKey) throw new Error('Set STRIPE_SECRET_KEY')
const webhookUrlIndex = process.argv.indexOf('--webhook-url')
const webhookUrl = webhookUrlIndex === -1 ? undefined : process.argv[webhookUrlIndex + 1]

// The events app/api/stripe/webhook/route.ts handles. scripts/run_stripe_website.sh forwards the same.
const WEBHOOK_EVENTS = ['customer.subscription.created', 'customer.subscription.updated', 'customer.subscription.deleted', 'invoice.paid']
const WEBHOOK_SECRET_PATH = path.join(homedir(), '.kube_stripe_webhook_secret')

// Stripe's API takes nested form fields, like features[subscription_update][products][0][prices][0].
const toFormFields = (value, key) => {
    if (Array.isArray(value)) return value.flatMap((item, index) => toFormFields(item, `${key}[${index}]`))
    if (value !== null && typeof value === 'object') return Object.entries(value).flatMap(([name, item]) => toFormFields(item, key === undefined ? name : `${key}[${name}]`))
    return [[key, String(value)]]
}

const stripeRequest = async ({ method, apiPath, params }) => {
    const form = new URLSearchParams(toFormFields(params, undefined))
    const url = method === 'GET' ? `https://api.stripe.com/v1/${apiPath}?${form}` : `https://api.stripe.com/v1/${apiPath}`
    const response = await fetch(url, { method, headers: { Authorization: `Bearer ${stripeSecretKey}` }, body: method === 'GET' ? undefined : form })
    const json = await response.json()
    if (!response.ok) throw new Error(`${method} /v1/${apiPath} failed: ${json.error.message}`)
    return json
}

console.log(`Setting up Stripe in ${stripeSecretKey.includes('_live_') ? 'LIVE' : 'test'} mode.\n`)

// -------------------------- A product and a monthly price for each plan --------------------------

// A price that already exists is left alone. If its amount no longer matches pricing.ts this stops:
// changing a price means moving every subscriber onto a new one, which is done by hand.
const { data: existingPrices } = await stripeRequest({ method: 'GET', apiPath: 'prices', params: { lookup_keys: plansInOrder.map((plan) => plan.id), active: true, limit: 100 } })
const priceOfPlanId = {}
for (const plan of plansInOrder) {
    const unitAmount = Math.round(plan.priceUsd * 100)
    const existingPrice = existingPrices.find((price) => price.lookup_key === plan.id)
    if (existingPrice !== undefined) {
        if (existingPrice.unit_amount !== unitAmount || existingPrice.currency !== 'usd' || existingPrice.recurring?.interval !== 'month') {
            throw new Error(`Price ${existingPrice.id} for ${plan.id} is ${existingPrice.unit_amount} ${existingPrice.currency} a ${existingPrice.recurring?.interval}, but pricing.ts says ${unitAmount} usd a month.`)
        }
        console.log(`${plan.name}: price ${existingPrice.id} on product ${existingPrice.product} is already set up.`)
        priceOfPlanId[plan.id] = existingPrice
        continue
    }
    const product = await stripeRequest({ method: 'POST', apiPath: 'products', params: { name: plan.name } })
    const price = await stripeRequest({ method: 'POST', apiPath: 'prices', params: { product: product.id, unit_amount: unitAmount, currency: 'usd', recurring: { interval: 'month' }, lookup_key: plan.id } })
    console.log(`${plan.name}: created product ${product.id} and price ${price.id} at ${unitAmount} cents a month.`)
    priceOfPlanId[plan.id] = price
}

// -------------------------- The two customer portal configurations --------------------------

// 'manage' is where a subscriber cancels, changes card, or sees invoices. It can't switch plans, so
// nobody can downgrade there and be credited for the rest of the month. 'upgrade' is only ever opened
// on the confirm page for one switch: it charges the new plan in full right away and starts a new
// month. Both are found again by their kube_portal metadata; see getPortalConfigurationId.
const featuresOfPurpose = {
    manage: {
        subscription_update: { enabled: false },
        subscription_cancel: { enabled: true, mode: 'at_period_end' },
        payment_method_update: { enabled: true },
        invoice_history: { enabled: true },
    },
    upgrade: {
        subscription_update: {
            enabled: true,
            default_allowed_updates: ['price'],
            products: plansInOrder.map((plan) => ({ product: priceOfPlanId[plan.id].product, prices: [priceOfPlanId[plan.id].id] })),
            proration_behavior: 'none',
            billing_cycle_anchor: 'now',
        },
        subscription_cancel: { enabled: false },
        payment_method_update: { enabled: true },
        invoice_history: { enabled: false },
    },
}
const { data: existingConfigurations } = await stripeRequest({ method: 'GET', apiPath: 'billing_portal/configurations', params: { active: true, limit: 100 } })
for (const [purpose, features] of Object.entries(featuresOfPurpose)) {
    const existingConfiguration = existingConfigurations.find((configuration) => configuration.metadata?.kube_portal === purpose)
    const params = { features, metadata: { kube_portal: purpose } }
    const configuration = await stripeRequest({ method: 'POST', apiPath: existingConfiguration === undefined ? 'billing_portal/configurations' : `billing_portal/configurations/${existingConfiguration.id}`, params })
    console.log(`Portal configuration '${purpose}': ${existingConfiguration === undefined ? 'created' : 'updated'} ${configuration.id}.`)
}

// -------------------------- The webhook, when a URL is given --------------------------

if (webhookUrl !== undefined) {
    const { data: existingEndpoints } = await stripeRequest({ method: 'GET', apiPath: 'webhook_endpoints', params: { limit: 100 } })
    const existingEndpoint = existingEndpoints.find((endpoint) => endpoint.url === webhookUrl)
    if (existingEndpoint !== undefined) {
        await stripeRequest({ method: 'POST', apiPath: `webhook_endpoints/${existingEndpoint.id}`, params: { enabled_events: WEBHOOK_EVENTS, disabled: false } })
        console.log(`Webhook ${existingEndpoint.id} to ${webhookUrl}: events updated. Its signing secret is unchanged.`)
    } else {
        const endpoint = await stripeRequest({ method: 'POST', apiPath: 'webhook_endpoints', params: { url: webhookUrl, enabled_events: WEBHOOK_EVENTS } })
        writeFileSync(WEBHOOK_SECRET_PATH, endpoint.secret, { mode: 0o600 })
        console.log(`Webhook ${endpoint.id} to ${webhookUrl}: created. Its signing secret is in ${WEBHOOK_SECRET_PATH}.`)
    }
}

console.log('\nDone.')
