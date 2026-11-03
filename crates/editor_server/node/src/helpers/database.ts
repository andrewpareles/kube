import { PostgrestClient, type PostgrestError } from '@supabase/postgrest-js'
import type { UIMessage } from 'ai'
import { HTTPException } from 'hono/http-exception'
import type { ContentfulStatusCode } from 'hono/utils/http-status'
import { env, isDev } from './env.ts'

// Every database operation the chat child makes. They go through PostgREST, with the service key and Supabase's own client, so
// an editor holds no database connection, and what has to happen in one transaction is a function in
// 0092_editor_database_functions.sql. A function raises 'PT<status>' for an error meant for the user, which keeps its status.
const postgrest = new PostgrestClient(`${env.SUPABASE_URL}/rest/v1`, {
    headers: {
        apikey: env.SUPABASE_SECRET_KEY,
        Authorization: `Bearer ${env.SUPABASE_SECRET_KEY}`,
    },
})

const result = async <Data>(
    request: PromiseLike<{ data: Data | null; error: PostgrestError | null; status: number }>,
) => {
    const { data, error, status } = await request
    if (error) {
        if (error.code.startsWith('PT'))
            throw new HTTPException(status as ContentfulStatusCode, { message: error.message })
        throw new Error(`Database request failed (status ${status}): ${error.message}`)
    }
    return data as Data
}

export const database = {
    // Returns the game's creator, who pays for the AI chat whoever is using it. Throws 403 or 410.
    browserAccess: async (profileId: string | null, inviteToken: string | null): Promise<string> =>
        result(
            postgrest.rpc('editor_browser_access', {
                input_game_id: env.GAME_ID,
                input_request_id: isDev ? null : env.ARBITRIUM_REQUEST_ID,
                input_profile_id: profileId,
                input_invite_token: inviteToken,
            }),
        ),

    chatMessages: async (): Promise<unknown[]> => {
        const rows = await result(
            postgrest
                .from('chat_messages')
                .select('message')
                .eq('game_id', env.GAME_ID)
                .order('seq'),
        )
        return rows.map((row) => row.message)
    },

    chatMessagesSinceSummary: async (): Promise<unknown[]> =>
        result(postgrest.rpc('chat_messages_since_summary', { input_game_id: env.GAME_ID })),

    runningReplies: async (): Promise<unknown[]> => {
        const rows = await result(
            postgrest
                .from('chat_messages')
                .select('message')
                .eq('game_id', env.GAME_ID)
                .eq('message->metadata->>isRunning', 'true'),
        )
        return rows.map((row) => row.message)
    },

    // A user message that is already saved, because the turn resumes it, is passed as null.
    startChatReply: async (userMessage: UIMessage | null, reply: UIMessage) => {
        await result(
            postgrest.rpc('start_chat_reply', {
                input_game_id: env.GAME_ID,
                input_user_message: userMessage,
                input_reply: reply,
            }),
        )
    },

    saveChatMessage: async (message: UIMessage) => {
        await result(
            postgrest
                .from('chat_messages')
                .update({ message })
                .eq('game_id', env.GAME_ID)
                .eq('message->>id', message.id),
        )
    },

    deleteChatMessage: async (messageId: string) => {
        await result(
            postgrest
                .from('chat_messages')
                .delete()
                .eq('game_id', env.GAME_ID)
                .eq('message->>id', messageId),
        )
    },

    creditsUsedToday: async (profileId: string) =>
        Number(await result(postgrest.rpc('credits_used_today', { input_profile_id: profileId }))),

    chargeChatUsage: async (call: {
        profileId: string
        messageId: string | null
        modelId: string
        inputTokens: number
        cacheWriteTokens: number
        cacheReadTokens: number
        outputTokens: number | null
        awsRequestId: string
        credits: number
    }) => {
        await result(
            postgrest.rpc('charge_chat_usage', {
                input_profile_id: call.profileId,
                input_game_id: env.GAME_ID,
                input_message_id: call.messageId,
                input_model_id: call.modelId,
                input_input_tokens: call.inputTokens,
                input_cache_write_tokens: call.cacheWriteTokens,
                input_cache_read_tokens: call.cacheReadTokens,
                input_output_tokens: call.outputTokens,
                input_aws_request_id: call.awsRequestId,
                input_credits: call.credits,
            }),
        )
    },
}
