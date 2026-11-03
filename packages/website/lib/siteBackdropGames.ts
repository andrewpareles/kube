import type { SiteBackdropGame } from '@/components/site/SiteGamesBackdrop'

// Set this to false to select the current top games instead of the saved background.
export const SHOULD_USE_HARDCODED_BACKDROP_GAMES = true

// Captured from the sign-in background on 2026-08-31, in its original order. The local PNG copies
// keep these thumbnails unchanged when games are edited, reordered, or removed.
export const hardcodedBackdropGames: SiteBackdropGame[] = [
    { gameId: 83, name: 'Steal the Thunderbolt', thumbnailUrl: '/site-backdrop/83.png' },
    { gameId: 67, name: 'Castaway Isles', thumbnailUrl: '/site-backdrop/67.png' },
    { gameId: 87, name: 'Hive', thumbnailUrl: '/site-backdrop/87.png' },
    { gameId: 72, name: 'Pedal Boats', thumbnailUrl: '/site-backdrop/72.png' },
    { gameId: 74, name: 'Kube Fun Park', thumbnailUrl: '/site-backdrop/74.png' },
    { gameId: 69, name: 'Who Killed the Forest Gnome', thumbnailUrl: '/site-backdrop/69.png' },
    { gameId: 73, name: 'Cake Bakeoff', thumbnailUrl: '/site-backdrop/73.png' },
    { gameId: 51, name: 'Sumo Push', thumbnailUrl: '/site-backdrop/51.png' },
    { gameId: 90, name: 'Snow Globe', thumbnailUrl: '/site-backdrop/90.png' },
    { gameId: 88, name: 'The Wooden Horse', thumbnailUrl: '/site-backdrop/88.png' },
    { gameId: 80, name: 'Bigfoot Photo Hunt', thumbnailUrl: '/site-backdrop/80.png' },
    { gameId: 69, name: 'Who Killed the Forest Gnome', thumbnailUrl: '/site-backdrop/69.png' },
    { gameId: 63, name: 'Zombie Infection', thumbnailUrl: '/site-backdrop/63.png' },
    { gameId: 89, name: 'Train Roof Run', thumbnailUrl: '/site-backdrop/89.png' },
    { gameId: 62, name: 'Zombie Outbreak', thumbnailUrl: '/site-backdrop/62.png' },
    { gameId: 92, name: 'The Elevator', thumbnailUrl: '/site-backdrop/92.png' },
    { gameId: 39, name: 'Clockwork Climb', thumbnailUrl: '/site-backdrop/39.png' },
    { gameId: 36, name: 'Lantern Leap', thumbnailUrl: '/site-backdrop/36.png' },
]
