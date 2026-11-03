import posthog from 'posthog-js'

// TODO: AI-PICKED-VALUE: e.kube.new is the subdomain for PostHog's managed proxy, so ad blockers don't drop events. PostHog says to avoid names like posthog or analytics that blockers look for.
posthog.init(process.env.NEXT_PUBLIC_POSTHOG_KEY!, { api_host: 'https://e.kube.new', ui_host: 'https://us.posthog.com', defaults: '2026-08-30', autocapture: false })
