import { notFound } from 'next/navigation'

// Everything is free for now, so there are no plans to show.
export default function Page() {
    notFound()
}

// import type { Metadata } from 'next'
// import { Check, Gift, type LucideIcon } from 'lucide-react'
// import { HoverTooltip } from '@/components/site/HoverTooltip'
// import { HatIcon } from '@/components/site/HatIcon'
// import { JsonLd } from '@/components/site/JsonLd'
// import { CREDIT_ICON, CREDIT_ICON_CLASS_NAME, getPercentOff, planConstsOfPlanId, plansInOrder, promotedPlanId, promotionEndText, type PlanConsts } from '@/lib/pricing'
// import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
// import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'
// import { PLANS_PATH } from '@/lib/siteNavigation'
// import { getUser } from '@/lib/siteUser'
// import { getStripePlanStateForProfile } from '@/lib/stripe'
// import { PlanButton } from './PlanButton'
//
// export const metadata: Metadata = getSitePageMetadata({
//     title: 'Plans',
//     description: 'Get a subscription to build more games and buy more items on kube.new.',
//     path: PLANS_PATH,
//     imageUrl: SITE_DEFAULT_IMAGE_PATH,
//     imageAlt: SITE_DEFAULT_IMAGE_ALT,
// })
//
// // The same list on every card, because these are true of either plan.
// const PLAN_CHECKS = [
//     'Experience a new way to build games.',
//     'Invite friends to edit live games with you.',
//     'Generate objects, scripts, meshes, and UI.',
//     'Credits never expire.',
// ]
//
// // Capitalised so JSX reads them as components, the same way the header does with its nav icons.
// const CreditIcon = CREDIT_ICON
//
// // Hovering the amount, its mark, or its /month says what it's for.
// const PlanAmountRow = ({ Icon, iconClassName, amount, purpose }: { Icon: LucideIcon, iconClassName: string, amount: number, purpose: string }) => (
//     <div className="plan-row">
//         <HoverTooltip
//             side="right"
//             className="plan-amount-tooltip"
//             trigger={
//                 <span className="plan-amount-trigger">
//                     <Icon className={`plan-row-icon ${iconClassName}`} aria-hidden="true" />
//                     <span className="plan-amount">{amount.toLocaleString()}</span>
//                     <span className="plan-amount-period">/month</span>
//                 </span>
//             }
//         >
//             Use
//             <span className="plan-amount-tooltip-amount">
//                 <Icon className={`plan-amount-tooltip-icon ${iconClassName}`} aria-hidden="true" />
//                 {amount.toLocaleString()}
//             </span>
//             per month to {purpose}.
//         </HoverTooltip>
//     </div>
// )
//
// type PlanState = Awaited<ReturnType<typeof getStripePlanStateForProfile>>
//
// const formatPlanDate = (seconds: number) => new Date(seconds * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
//
// // Checked in the same order openPlan routes a click, so a button always says what it will do. Two are
// // shown but can't be pressed: switching while set to cancel, until they renew, and managing a plan
// // while a downgrade is waiting, until they take it back. The arrow only goes on buttons that open one
// // of Stripe's pages.
// const getPlanButton = ({ plan, planState }: { plan: PlanConsts, planState: PlanState }) => {
//     if (planState === undefined) return { label: `Get ${plan.name}`, note: undefined, isArrowShown: true, isDisabled: false }
//     const { subscription, schedule } = planState
//     if (plan.id === schedule?.planId) {
//         return { label: "Don't downgrade", note: `Starts ${formatPlanDate(planState.periodEnd)}`, isArrowShown: false, isDisabled: false }
//     }
//     if (subscription.status === 'past_due') return { label: 'Fix payment', note: undefined, isArrowShown: true, isDisabled: false }
//     if (plan.id === planState.planId) {
//         if (subscription.cancel_at !== null) {
//             return { label: "Don't cancel", note: `Ends ${formatPlanDate(subscription.cancel_at)}`, isArrowShown: true, isDisabled: false }
//         }
//         if (schedule?.planId !== undefined) return { label: 'Manage plan', note: undefined, isArrowShown: false, isDisabled: true }
//         return { label: 'Manage plan', note: undefined, isArrowShown: true, isDisabled: false }
//     }
//     if (subscription.cancel_at !== null) return { label: 'Renew to switch', note: undefined, isArrowShown: false, isDisabled: true }
//     const isUpgrade = plan.priceUsd > planConstsOfPlanId[planState.planId].priceUsd
//     return { label: isUpgrade ? 'Upgrade' : 'Downgrade', note: undefined, isArrowShown: isUpgrade, isDisabled: false }
// }
//
// const PlanCard = ({ plan, planState, next }: { plan: PlanConsts, planState: PlanState, next: string }) => (
//     <section className={`plan-card${plan.isPopular ? ' plan-card-popular' : ''}`}>
//         {plan.isPopular && <span className="plan-badge">POPULAR</span>}
//         <h2 className="plan-name">{plan.name}</h2>
//
//         {/* The old price reads first and smaller, so the eye lands on what it costs now. */}
//         <p className="plan-price">
//             {plan.wasPriceUsd !== null && (
//                 <span className="plan-price-was">${plan.wasPriceUsd.toFixed(2)}</span>
//             )}
//             <span className="plan-price-now">${plan.priceUsd.toFixed(2)}</span>
//             <span className="plan-price-period">/month</span>
//         </p>
//
//
//         {/* The credits and the free hat sit apart from the things true of either plan. */}
//         <div className="plan-list-group plan-amounts">
//             <PlanAmountRow Icon={CreditIcon} iconClassName={CREDIT_ICON_CLASS_NAME} amount={plan.creditsPerMonth} purpose="make things and buy items" />
//             <div className="plan-row">
//                 <span className="plan-amount-trigger">
//                     <HatIcon className="plan-row-icon text-kube-blue" />
//                     <span>Get a free hat.</span>
//                 </span>
//             </div>
//         </div>
//
//         <div className="plan-list-group plan-checks">
//             {PLAN_CHECKS.map((check) => (
//                 <div key={check} className="plan-row">
//                     <Check className="plan-row-icon plan-check" aria-hidden="true" />
//                     <span>{check}</span>
//                 </div>
//             ))}
//         </div>
//
//         <PlanButton planId={plan.id} next={next} {...getPlanButton({ plan, planState })} />
//     </section>
// )
//
// const PromoLine = () => {
//     if (promotedPlanId === null) return null
//     const plan = planConstsOfPlanId[promotedPlanId]
//     const percentOff = getPercentOff({ plan })
//     if (percentOff === null) return null
//
//     return (
//         <p className="plans-promo">
//             <Gift className="plans-promo-icon" aria-hidden="true" />
//             <span className="plans-promo-offer">{percentOff}% off {plan.name}</span> {promotionEndText}.
//         </p>
//     )
// }
//
// // next is where checkout sends the browser once it's done. It is carried here from wherever the
// // visitor clicked their balance.
// export default async function Page({ searchParams }: { searchParams: Promise<{ next?: string }> }) {
//     const { next } = await searchParams
//     const user = await getUser()
//     const planState = user === null ? undefined : await getStripePlanStateForProfile({ profileId: user.id })
//
//     return (
//         <>
//             <JsonLd value={[getBreadcrumbJsonLd({ path: PLANS_PATH, pageName: 'Plans' })]} />
//             <div className="plans-page">
//                 {/* Only for screen readers and search engines: the page opens with the cube instead. */}
//                 <h1 className="sr-only">Plans</h1>
//
//                 {/* The offer and the tagline read as one opening block. */}
//                 <div className="plans-intro">
//                     <PromoLine />
//                     <p className="plans-mark-caption">Make the cool, creative, fun games you always wanted to.</p>
//                     {/* <p className="plans-mark-caption">Build the fun, creative, and cool games you always wanted to.</p> */}
//                 </div>
//
//                 <div className="plans-grid">
//                     {plansInOrder.map((plan) => <PlanCard key={plan.id} plan={plan} planState={planState} next={next === undefined ? PLANS_PATH : next} />)}
//                 </div>
//
//             </div>
//         </>
//     )
// }
