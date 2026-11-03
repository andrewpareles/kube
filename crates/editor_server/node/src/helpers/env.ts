import { z } from 'zod'

// The child serves authenticated chat and calls Rust's tools inside the same container.
export const isDev = process.argv.includes('--dev')
const parsed = z
    .object({
        EDITOR_SERVER_SECRET: z.string().min(32),
        ARBITRIUM_REQUEST_ID: z.string().min(1),
        GAME_ID: z.coerce.number().int().positive().safe(),
        EDITOR_SERVER_PORT: z.coerce.number().int().min(1).max(65535),
        EDITOR_CHAT_PORT: z.coerce.number().int().min(1).max(65535),
        SITE_URL: z.url(),
        SUPABASE_URL: z.url(),
        SUPABASE_SECRET_KEY: z.string().min(1),
        S3_ACCESS_KEY_ID: z.string().min(1),
        S3_SECRET_ACCESS_KEY: z.string().min(1),
        AWS_REGION: z.string().min(1),
        AWS_BEARER_TOKEN_BEDROCK: z.string().min(1),
        AZURE_OPENAI_API_KEY: z.string().min(1),
    })
    .safeParse(process.env)
if (!parsed.success)
    throw new Error(
        `Missing or invalid Node worker configuration: ${parsed.error.issues.map((issue) => issue.path.join('.')).join(', ')}.`,
    )
export const env = parsed.data
