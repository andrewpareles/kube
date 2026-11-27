import fs from 'node:fs/promises'
import path from 'node:path'
import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { PlayScene } from './PlayScene'

export const metadata: Metadata = {
    title: 'Play',
    robots: { index: false, follow: false },
    referrer: 'no-referrer',
}

// A .kube file at the repository root is one page's game until the play-session API hands these out, so the id names the
// file and nothing else. The pattern keeps an id from naming anything outside the root.
const worldFilePathOfGameId = (gameId: string) =>
    /^[a-z0-9]+(-[a-z0-9]+)*$/.test(gameId) ? path.join(process.cwd(), '..', '..', `${gameId}.kube`) : null

const Page = async ({ params }: { params: Promise<{ game_id: string }> }) => {
    const { game_id: gameId } = await params
    const worldFilePath = worldFilePathOfGameId(gameId)
    if (worldFilePath === null) notFound()
    // The play session migrations and loads this the same way the editor does when it opens a world.
    const worldJson = await fs.readFile(worldFilePath, 'utf8').catch(() => null)
    if (worldJson === null) notFound()

    return <PlayScene worldJson={worldJson} />
}

export default Page
