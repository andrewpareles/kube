import { z } from 'zod'

export const toolImageSchema = z.object({ media_type: z.string(), base64: z.string() })
export type ToolImage = z.infer<typeof toolImageSchema>

export const toolResultSchema = z.object({
    is_error: z.boolean(),
    text: z.string(),
    images: z.array(toolImageSchema),
})
