export const usernameMinLength = 4
export const usernameMaxLength = 16

/**
 * What the server says about a name somebody wants: nobody has it, somebody has it, or it is not a
 * name they are allowed to have. The last one carries the sentence to show them, because the reason
 * differs and only the server knows it.
 */
export type UsernameAvailability =
    | { type: 'available' }
    | { type: 'taken' }
    | { type: 'problem', problem: string }

/**
 * What is wrong with a name somebody typed, or null if nothing is. The table checks the same three
 * things, so a name that gets past this is one the table will take as well. It exists so the person
 * typing hears about a bad name while they are still on the field rather than after pressing the
 * button.
 */
export const getUsernameProblem = (username: string) => {
    if (username.length < usernameMinLength) return `At least ${usernameMinLength} characters.`
    if (username.length > usernameMaxLength) return `At most ${usernameMaxLength} characters.`
    // Anything else would let two names that read exactly alike, like "bob" and "bob ", be held by
    // two different people.
    if (!/^[A-Za-z0-9]+$/.test(username)) return 'Sorry, letters and numbers only.'
    return null
}
