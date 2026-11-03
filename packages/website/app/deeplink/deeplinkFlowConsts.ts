// These constants keep the browser handoff paths in one place.

// The login modal is open while the url has this param, and its value is where to go once they are signed in.
export const LOGIN_SEARCH_PARAM = 'login'

export const GAME_PATH = (gameId: number) => `/games/${encodeURIComponent(String(gameId))}`

// login page with the next location entered
export const getLoginUrl = ({ next }: { next: string }) => `/?${LOGIN_SEARCH_PARAM}=${encodeURIComponent(next)}`
