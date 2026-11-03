import { readFileSync } from 'node:fs'
import path from 'node:path'
import { hasProfanity, initSync } from './gen/profanity/profanity.js'

/**
 * The one profanity matcher the site uses, for usernames and for the words people put on a game.
 *
 * It reads through the tricks people use to get a word past a plain list, so sh1t and fvck are
 * caught along with the word itself.
 *
 * Everything that reaches this lives on the server, which is what keeps the word list out of what
 * gets sent to the page. It is not the last word either: anything that writes to Supabase without
 * going through these helpers skips it entirely.
 *
 * The rules themselves come from the `profanity` Rust crate, the same one the game server censors
 * chat with, built to WebAssembly by the `build:wasm` step. The file is read from the working
 * directory the same way the texture transcoder is, because the copy Next puts beside the bundled
 * code is the one it serves to browsers; `outputFileTracingIncludes` in `next.config.ts` is what
 * ships it. Starting it up reads the file and returns, with nothing to await, so every caller
 * stays an ordinary function.
 */
initSync({ module: readFileSync(path.join(process.cwd(), 'lib/gen/profanity/profanity_bg.wasm')) })

export { hasProfanity }
