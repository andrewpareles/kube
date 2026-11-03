This is a recap of all the secrets we store, where we store them, and how we access them.

Primarily written by AI.

# Secrets

All secrets are stored in Infisical (project `kube-new`) and synced to Vercel and GitHub as secrets. Change them in Infisical, not in Vercel or GitHub, since the next sync overwrites changes made there.

Public settings aren't secrets. They live in `packages/website/.env`, in git.

These are the secrets we store in Infisical, and where they're used.

## Production `/vercel`, synced to Vercel (Production and Preview)

- `DRIZZLE_DATABASE_URL`: website
- `STRIPE_SECRET_KEY`: website
- `STRIPE_WEBHOOK_SECRET`: website
- `EDGEGAP_API_TOKEN`: website
- `EDGEGAP_WEBHOOK_SECRET`: website
- `WEBSITE_S3_ACCESS_KEY_ID` (AWS user `kube-new-website`): website
- `WEBSITE_S3_SECRET_ACCESS_KEY` (AWS user `kube-new-website`): website
- `EDITOR_SERVER_SECRET`: website and editor
- `RESEND_API_KEY` (a Resend API key with sending access): website, and the editor for the email a creator gets when their game is first published
- `SUPABASE_SECRET_KEY`: game server and editor
- `S3_ACCESS_KEY_ID` (AWS user `kube-new-servers`): game server and editor
- `S3_SECRET_ACCESS_KEY` (AWS user `kube-new-servers`): game server and editor
- `AWS_BEARER_TOKEN_BEDROCK` (a Bedrock API key): editor
- `AZURE_OPENAI_API_KEY` (a key for the `useglass-foundry` Azure resource, whose `gpt-image-2.5-sunburst` deployment draws the AI thumbnails): editor
- `QUIC_SERVER_KEY_DER_BASE64`: game server and editor

The website passes the game server and editor theirs as hidden env vars when it starts them on Edgegap. `secretEnvKeysOfServer` in `packages/website/lib/edgegapApi.ts` lists what each one gets, under the same names.

## Production `/github`, synced to GitHub Actions

Only `.github/workflows/release-packaging.yml` reads these.

- `APPLE_CERTIFICATE`: signing the Mac app
- `APPLE_CERTIFICATE_PASSWORD`: signing the Mac app
- `APPLE_API_KEY`: notarizing the Mac app
- `APPLE_API_ISSUER`: notarizing the Mac app
- `APPLE_API_KEY_P8_BASE64`: notarizing the Mac app
- `AZURE_CLIENT_ID`: signing the Windows app
- `AZURE_TENANT_ID`: signing the Windows app
- `ARTIFACT_SIGNING_ENDPOINT`: signing the Windows app
- `ARTIFACT_SIGNING_ACCOUNT`: signing the Windows app
- `ARTIFACT_SIGNING_PROFILE`: signing the Windows app
- `CARGO_PACKAGER_SIGN_PRIVATE_KEY`: signing desktop updates
- `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`: signing desktop updates
- `EDGEGAP_REGISTRY_USERNAME`: logging in to Edgegap's image registry to push server images
- `EDGEGAP_REGISTRY_TOKEN`: logging in to Edgegap's image registry to push server images
- `EDGEGAP_API_TOKEN`: pointing the `prod` Edgegap versions at the new images and deleting old ones

`EDGEGAP_API_TOKEN` here is a reference to the `/vercel` one, `${prod.vercel.EDGEGAP_API_TOKEN}`, so it's changed in one place.

## Development, for your computer

Development imports Production `/vercel` and has its own:

- `STRIPE_SECRET_KEY`: Stripe's sandbox key, in place of the live one
- `SITE_URL`: `http://localhost:3000`

`packages/website/.env.local` holds these on your computer, and git ignores it. Install the CLI with `npm install -g @infisical/cli`, run `infisical login` once, then write the file from the repo root:

```bash
infisical export --env=dev --projectId=765b78c0-459c-485a-9320-c0c875c3e43a --output-file=packages/website/.env.local
```

To test Edgegap webhooks from your computer, run ngrok and add your ngrok URL as a personal `EDGEGAP_WEBHOOK_BASE_URL` secret in Development, then export again.

Local playtests get `QUIC_SERVER_KEY_DER_BASE64` from `.env.local` through the website, like the other editor secrets. The server tests that start a real server, `join_tests` and `quic_cert_bundle_tests`, need it too, so run them with `infisical run --env=dev -- cargo test -p server`.

## Not secrets

These are public, so they stay in `packages/website/.env` and in git: the Supabase publishable key, the PostHog key, the Google client ID, the updater's public key (`UPDATER_PUBLIC_KEY`), and the QUIC cert.
