import { getDefaultCharacterData } from '@/lib/siteAvatar'

// Avatars on every page are drawn from this, and it only changes when the DefaultCharacter asset is published again.
// TODO: AI-PICKED-VALUE: Browsers keep it for a minute and the CDN for 10 minutes, so a new version shows up within about 10 minutes.
export async function GET() {
    return Response.json(await getDefaultCharacterData(), {
        headers: { 'Cache-Control': 'public, max-age=60, s-maxage=600' },
    })
}
