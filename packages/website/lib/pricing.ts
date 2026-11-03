import { Coins, type LucideIcon } from 'lucide-react'

export const CREDIT_ICON: LucideIcon = Coins
export const CREDIT_ICON_CLASS_NAME = 'text-kube-blue'

// An account with no subscription is free, so there is no plan id for it. Anywhere a user's
// current plan is held, the type is PlanId | null.
// These ids are written into the database and into Stripe's price lookup keys, so renaming one is
// a migration and a Stripe change. The name a visitor reads is `name` below and is free to change.
export type PlanId = 'pro' | 'max'

export type PlanConsts = {
    id: PlanId
    name: string
    priceUsd: number
    // Drawn with a line through it beside the price.
    wasPriceUsd: number | null
    // Granted in full every time an invoice is paid. Nothing expires, so it adds to what is left.
    creditsPerMonth: number
    isPopular: boolean
}

export const planConstsOfPlanId: { [id in PlanId]: PlanConsts } = {
    pro: {
        id: 'pro',
        name: 'Pro',
        priceUsd: 19.99,
        wasPriceUsd: 24.99,
        creditsPerMonth: 2500,
        isPopular: true,
    },
    max: {
        id: 'max',
        name: 'Max',
        priceUsd: 49.99,
        wasPriceUsd: 64.99,
        creditsPerMonth: 6500,
        isPopular: false,
    },
}

export const plansInOrder: PlanConsts[] = [planConstsOfPlanId.pro, planConstsOfPlanId.max]

// The plan named in the line above the cards. Null runs no promotion and hides that line. The
// percentage is worked out from the two prices rather than written down, so it cannot claim a
// bigger saving than the prices actually give.
export const promotedPlanId: PlanId | null = 'pro'
// How long the promotion runs, as the words that finish that line.
export const promotionEndText = 'until the end of 2026'

export const getPercentOff = ({ plan }: { plan: PlanConsts }) =>
    plan.wasPriceUsd === null ? null : Math.round((1 - plan.priceUsd / plan.wasPriceUsd) * 100)

// One credit stands for one cent of model spend at list price, which is what every allotment above
// is quoted against: Pro's 2500 credits is $25 of model time for $19.99.
// The editor server charges credits from its own process and cannot import this file, so it keeps
// its own copy of this number next to the model rates. The two must agree.
export const USD_PER_CREDIT = 0.01
