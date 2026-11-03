export const displayedDescription = (description: string | null) => description?.trim().replace(/(?:[^\S\n]*\n){2,}/g, '\n')
