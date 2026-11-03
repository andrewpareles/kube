import type { ObjectTypeDocField } from '@/lib/objectTypeDocs'
import { DocsMarkdown } from '@/components/docs/DocsMarkdown'

export const DocsTypeTable = ({ fields, typeAnchorOfName, isMuted }: { fields: ObjectTypeDocField[]; typeAnchorOfName: { [name: string]: string }; isMuted: boolean }) => (
    <div className={isMuted ? 'mt-4 overflow-x-auto border-y border-gray-100' : 'mt-4 overflow-x-auto border-y border-gray-200'}>
        <table className="w-full min-w-[720px] table-fixed border-collapse text-left text-sm">
            <thead className={isMuted ? 'bg-gray-50 text-xs uppercase tracking-wide text-gray-400' : 'bg-gray-50 text-xs uppercase tracking-wide text-gray-500'}>
                <tr>
                    <th className="w-48 px-4 py-3 font-semibold">Field</th>
                    <th className={(fields.some((field) => functionTypePartsOfText(field.type) !== null) ? 'w-64' : 'w-48') + ' px-4 py-3 font-semibold'}>Type</th>
                    <th className="px-4 py-3 font-semibold" aria-label="Docstring"></th>
                </tr>
            </thead>
            <tbody className={isMuted ? 'divide-y divide-gray-50' : 'divide-y divide-gray-100'}>
                {fields.map((field) => (
                    <tr key={field.field}>
                        <td className="overflow-x-auto whitespace-nowrap px-4 py-3 align-top">{field.field}</td>
                        <td className={isMuted ? 'px-4 py-3 align-top font-mono text-xs text-gray-500' : 'px-4 py-3 align-top font-mono text-xs text-gray-700'}>
                            <DocsTypeText text={field.type} typeAnchorOfName={typeAnchorOfName} isMuted={isMuted} />
                        </td>
                        <td className={isMuted ? 'px-4 py-3 align-top text-gray-400' : 'px-4 py-3 align-top text-gray-600'}>
                            <DocsMarkdown text={field.docstring} typeAnchorOfName={typeAnchorOfName} isMuted={isMuted} />
                        </td>
                    </tr>
                ))}
            </tbody>
        </table>
    </div>
)

export const DocsTypeText = ({ text, typeAnchorOfName, isMuted }: { text: string; typeAnchorOfName: { [name: string]: string }; isMuted: boolean }) => {
    const functionParts = functionTypePartsOfText(text)
    if (functionParts === null) return <DocsMarkdown text={text} typeAnchorOfName={typeAnchorOfName} isMuted={isMuted} />

    return (
        <div className="space-y-2">
            {(inputLinesOfText(functionParts.inputs).length === 0 ? ['()'] : inputLinesOfText(functionParts.inputs)).map((inputLine, inputLineIndex) => (
                <DocsMarkdown key={inputLineIndex} text={inputLine} typeAnchorOfName={typeAnchorOfName} isMuted={isMuted} />
            ))}
            <div className={isMuted ? 'text-gray-400' : 'text-gray-500'}><span className="inline-block w-[2ch] text-center" aria-label="=>">&rarr;</span></div>
            <DocsMarkdown text={functionParts.returns} typeAnchorOfName={typeAnchorOfName} isMuted={isMuted} />
        </div>
    )
}

const inputLinesOfText = (text: string): string[] => {
    if (text.trim() === '') return []
    const parts = splitTopLevel(text, ',').filter((part) => !isHiddenInput(part))
    return parts.map((part, partIndex) => partIndex === parts.length - 1 ? part : `${part},`)
}

const isHiddenInput = (text: string): boolean => /^__[A-Za-z0-9_]+__\?:/.test(text.trim())

const functionTypePartsOfText = (text: string): { typeParameters: string; inputs: string; returns: string } | null => {
    const arrowIndex = topLevelArrowIndexOfText(text)
    if (arrowIndex === -1) return null

    const left = text.slice(0, arrowIndex).trim()
    const paramsStart = topLevelCharacterIndex(left, '(')
    if (paramsStart === -1 || !left.endsWith(')')) return null

    return {
        typeParameters: left.slice(0, paramsStart).trim(),
        inputs: left.slice(paramsStart + 1, -1).trim(),
        returns: text.slice(arrowIndex + 2).trim(),
    }
}

const topLevelArrowIndexOfText = (text: string): number => {
    for (let index = 0, parens = 0, brackets = 0, braces = 0, angles = 0; index < text.length - 1; index++) {
        if (text[index] === '=' && text[index + 1] === '>' && parens === 0 && brackets === 0 && braces === 0 && angles === 0) return index
        const char = text[index]
        if (char === '(') parens++
        else if (char === ')') parens--
        else if (char === '[') brackets++
        else if (char === ']') brackets--
        else if (char === '{') braces++
        else if (char === '}') braces--
        else if (char === '<') angles++
        else if (char === '>' && text[index - 1] !== '=') angles = Math.max(0, angles - 1)
    }
    return -1
}

const topLevelCharacterIndex = (text: string, target: string): number => {
    for (let index = 0, angles = 0; index < text.length; index++) {
        const char = text[index]
        if (char === target && angles === 0) return index
        if (char === '<') angles++
        else if (char === '>' && text[index - 1] !== '=') angles = Math.max(0, angles - 1)
    }
    return -1
}

const splitTopLevel = (text: string, separator: string): string[] => {
    const parts: string[] = []
    let partStart = 0
    for (let index = 0, parens = 0, brackets = 0, braces = 0, angles = 0; index < text.length; index++) {
        const char = text[index]
        if (char === separator && parens === 0 && brackets === 0 && braces === 0 && angles === 0) {
            parts.push(text.slice(partStart, index).trim())
            partStart = index + 1
            continue
        }
        if (char === '(') parens++
        else if (char === ')') parens--
        else if (char === '[') brackets++
        else if (char === ']') brackets--
        else if (char === '{') braces++
        else if (char === '}') braces--
        else if (char === '<') angles++
        else if (char === '>' && text[index - 1] !== '=') angles = Math.max(0, angles - 1)
    }
    parts.push(text.slice(partStart).trim())
    return parts
}
