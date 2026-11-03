#!/usr/bin/env bash
set -euo pipefail

# The website plus the Stripe listener that carries webhooks back to it. Stripe cannot reach
# localhost on its own, so without the listener a test payment goes through in Stripe and nothing
# on this machine ever hears about it - no coins, no subscription row.
#
# The listener prints a signing secret of its own, different from the one in the Stripe dashboard
# and new on most runs, so it is read here and handed to the website rather than written down in a
# file that would go stale.

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v stripe >/dev/null 2>&1; then
    echo "The Stripe CLI is not installed. Download the release for this machine from" >&2
    echo "https://github.com/stripe/stripe-cli/releases, check it against the checksums file" >&2
    echo "published beside it, and put the binary on PATH." >&2
    echo "(brew install stripe/stripe-cli/stripe builds from source and needs current Xcode" >&2
    echo "Command Line Tools, which the prebuilt binary does not.)" >&2
    exit 1
fi

# The key is read from the website's own env file, so there is no separate stripe login to keep
# signed in and nothing to get out of step with the key the site itself uses. It is the sandbox key in
# .env.local, which only your computer has; the live key is in Vercel.
STRIPE_SECRET_KEY="$(. "$REPO_ROOT/packages/website/.env.local" && printf '%s' "${STRIPE_SECRET_KEY:-}")"
if [[ -z "${STRIPE_SECRET_KEY:-}" ]]; then
    echo "No STRIPE_SECRET_KEY in packages/website/.env.local" >&2
    exit 1
fi

# Only the events the webhook actually handles, the same four scripts/setup_stripe.mjs subscribes
# production to. The CLI insists on being told, and a narrower list means a run's output is the
# payments being tested rather than everything the account does.
STRIPE_EVENTS="customer.subscription.created,customer.subscription.updated,customer.subscription.deleted,invoice.paid"

WEBHOOK_PATH="/api/stripe/webhook"
LISTEN_LOG="$(mktemp -t kube_stripe_listen)"

# Every path out of here takes both processes with it, so a second run never finds the port or the
# listener still held by the first.
stripe_pid=""
website_pid=""
stop() {
    trap - INT TERM EXIT
    [[ -n "$website_pid" ]] && kill "$website_pid" 2>/dev/null || true
    [[ -n "$stripe_pid" ]] && kill "$stripe_pid" 2>/dev/null || true
    rm -f "$LISTEN_LOG"
}
trap stop INT TERM EXIT

echo "Starting the Stripe listener, forwarding to localhost:3000${WEBHOOK_PATH}"
stripe listen --api-key "$STRIPE_SECRET_KEY" --events "$STRIPE_EVENTS" \
    --forward-to "localhost:3000${WEBHOOK_PATH}" >"$LISTEN_LOG" 2>&1 &
stripe_pid=$!

# Wait for the secret it prints. Its absence means the listener failed, and its own output says
# why, so that is what gets shown.
STRIPE_WEBHOOK_SECRET=""
for _ in $(seq 1 40); do
    if ! kill -0 "$stripe_pid" 2>/dev/null; then
        echo "The Stripe listener stopped:" >&2
        cat "$LISTEN_LOG" >&2
        exit 1
    fi
    STRIPE_WEBHOOK_SECRET="$(grep -oE 'whsec_[A-Za-z0-9]+' "$LISTEN_LOG" | head -1 || true)"
    [[ -n "$STRIPE_WEBHOOK_SECRET" ]] && break
    sleep 0.25
done

if [[ -z "$STRIPE_WEBHOOK_SECRET" ]]; then
    echo "The Stripe listener never printed a signing secret:" >&2
    cat "$LISTEN_LOG" >&2
    exit 1
fi

echo "Webhook signing secret: ${STRIPE_WEBHOOK_SECRET:0:12}..."
echo "Test card 4242 4242 4242 4242, any future expiry, any CVC."
echo

# The listener keeps running in the background; its output joins the website's.
tail -f "$LISTEN_LOG" &

cd "$REPO_ROOT/packages/website"
STRIPE_WEBHOOK_SECRET="$STRIPE_WEBHOOK_SECRET" npm run --ignore-scripts dev -- "$@" &
website_pid=$!
wait "$website_pid"
