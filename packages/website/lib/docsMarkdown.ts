const codeSpanSplitRegex = /(`[^`]*`)/g
const wordRegex = /\b[A-Za-z_][A-Za-z0-9_]*\b/g

// Generated TypeScript uses angle brackets, which must stay text instead of becoming HTML tags.
export const getDocsMarkdownWithTypeLinks = (text: string, typeAnchorOfName: { [name: string]: string }) =>
    text.split(codeSpanSplitRegex).map((textPart) => {
        if (textPart.startsWith('`') && textPart.endsWith('`')) return textPart
        const textWithoutHtmlTags = textPart.replace(/<(?=[A-Za-z_])/g, '\\<')
        return textWithoutHtmlTags.replace(wordRegex, (word) => {
            const typeAnchor = typeAnchorOfName[word]
            if (typeAnchor === undefined) return word
            return `[${word}](#${typeAnchor})`
        })
    }).join('')
