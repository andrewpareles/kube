'use client'

import { useEffect, useState } from 'react'
import { getUsernameAvailability } from '@/app/server-actions/username-available'
import { getUsernameProblem, type UsernameAvailability } from '@/lib/siteUsernames'

// Long enough that a name is only looked up once the typing has stopped, short enough that the
// answer is there by the time the eye has reached the next field.
const availabilityLookupDelayMs = 400

// The server's three answers, plus the two the field is in before there is one to show.
export type UsernameFieldState =
    | { type: 'empty' }
    | { type: 'looking' }
    | UsernameAvailability


export const useUsernameAvailability = (username: string): UsernameFieldState => {
    const [checkedAvailability, setCheckedAvailability] = useState<{ username: string, availability: UsernameAvailability } | null>(null)
    const problem = username === '' ? null : getUsernameProblem(username)

    useEffect(() => {
        if (username === '' || getUsernameProblem(username) !== null) return

        const timeoutId = window.setTimeout(async () => {
            setCheckedAvailability({ username, availability: await getUsernameAvailability(username) })
        }, availabilityLookupDelayMs)

        return () => window.clearTimeout(timeoutId)
    }, [username])

    if (username === '') return { type: 'empty' }
    if (problem !== null) return { type: 'problem', problem }
    if (checkedAvailability === null || checkedAvailability.username !== username) return { type: 'looking' }
    return checkedAvailability.availability
}
