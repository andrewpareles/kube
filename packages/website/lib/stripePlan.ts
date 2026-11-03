import { plansInOrder } from './pricing.ts'

// A Stripe price names its plan through its lookup key, which is the plan id itself.
export const getPlanIdForLookupKey = (lookupKey: string | null | undefined) => plansInOrder.find((plan) => plan.id === lookupKey)?.id
