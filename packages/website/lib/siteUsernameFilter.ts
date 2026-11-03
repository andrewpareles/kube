import { hasProfanity } from './siteProfanityFilter.ts'

/**
 * Names nobody is allowed to take, on top of the rules about how a name has to be written.
 *
 * This is only ever reached from the server actions the sign up form calls, which is what keeps the
 * word list out of what gets sent to the page. It is not the last word either: anything that talks
 * to Supabase without going through the form skips it entirely, so the table's own constraints stay
 * the only thing that is actually guaranteed.
 */

// Words that would let somebody be taken for the site itself, or for one of the people running it.
// Matched whole rather than inside a name, so adminah is fine and admin is not.
const reservedUsernames = [
    'admin', 'administrator', 'moderator', 'mod', 'staff', 'support', 'help', 'contact',
    'kube', 'kubenew', 'official', 'system', 'root', 'security',
    // The site's own sections, so a name can never read as a part of the site.
    'api', 'blog', 'docs', 'game', 'games', 'login', 'profile', 'settings',
]

export const getUsernameFilterProblem = (username: string) => {

    if (reservedUsernames.includes(username.toLowerCase())) return 'That username is not allowed.'
    
    if (hasProfanity(username)) return 'That username is not allowed.'
    return null
}
