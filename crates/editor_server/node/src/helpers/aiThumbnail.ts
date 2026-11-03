import { createAzure } from '@ai-sdk/azure'
import { generateImage } from 'ai'
import { z } from 'zod'
import { env } from './env.ts'

// Publishing redraws the screenshot from the game's fallback camera as a polished thumbnail. OpenAI's model runs on
// Azure, on the useglass-foundry resource's deployment of the same name, so it bills the Azure credits.
const azure = createAzure({ resourceName: 'useglass-foundry', apiKey: env.AZURE_OPENAI_API_KEY })

// TODO: AI-PICKED-VALUE: GPT Image 2.5 Sunburst led the image editing leaderboards in October 2026. 2048x1152 is the
// biggest 16:9 size OpenAI doesn't call experimental, so the thumbnail stays sharp on retina screens.
const AI_THUMBNAIL_MODEL_ID = 'gpt-image-2.5-sunburst'
const AI_THUMBNAIL_SIZE = '2048x1152'

export const aiThumbnailRequestSchema = z.object({
    name: z.string(),
    description: z.string().nullable(),
    screenshot_png_base64: z.string(),
})

export const generateAiThumbnail = async ({
    name,
    description,
    screenshot_png_base64,
}: z.infer<typeof aiThumbnailRequestSchema>) => {
    const { image } = await generateImage({
        model: azure.image(AI_THUMBNAIL_MODEL_ID),
        prompt: {
            images: [screenshot_png_base64],
            text: `This is a screenshot of "${name}", a multiplayer game made with kube.new, a 3D game engine with a blocky art style.${description === null ? '' : ` The game's description: ${description}`}

Turn it into this game's thumbnail: polished, professional key art like the thumbnails on other platforms.
- Stay true to the game. Keep the same world, objects, characters, colors and blocky art style, from roughly the same view. Characters keep their blocky shapes, faces and outfits. Add nothing that isn't in the game.
- Make it exciting: a moment of play, with any characters in lively poses doing what the game is about, cinematic lighting, rich color and depth of field.
- Remove name tags, health bars and any other interface. Add no text, titles or logos.`,
        },
        size: AI_THUMBNAIL_SIZE,
        providerOptions: { openai: { quality: 'high' } },
    })
    return Buffer.from(image.base64, 'base64')
}
