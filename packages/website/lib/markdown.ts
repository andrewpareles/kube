import { readFile } from 'node:fs/promises'
import path from 'node:path'

const FRONTMATTER_START = '---\n'
const FRONTMATTER_END = '\n---\n'
const websiteDirPath = path.basename(process.cwd()) === 'website' ? process.cwd() : path.join(process.cwd(), 'packages/website')

export const getWebsiteFilePath = (...relativePathSegments: string[]) => path.join(websiteDirPath, ...relativePathSegments)

// Markdown content stays as text for Streamdown, while frontmatter supplies page and listing metadata.
export const readMarkdownFile = async (markdownFilePath: string) => {
    const fileText = await readFile(markdownFilePath, 'utf8')
    if (!fileText.startsWith(FRONTMATTER_START)) {
        throw new Error(`Markdown file ${markdownFilePath} is missing frontmatter.`)
    }

    const frontmatterEndIndex = fileText.indexOf(FRONTMATTER_END, FRONTMATTER_START.length)
    if (frontmatterEndIndex === -1) {
        throw new Error(`Markdown file ${markdownFilePath} is missing the closing frontmatter marker.`)
    }

    const metadataText = fileText.slice(FRONTMATTER_START.length, frontmatterEndIndex)
    const metadataValueOfKey: { [key: string]: string } = {}

    for (const metadataLine of metadataText.split('\n')) {
        const trimmedMetadataLine = metadataLine.trim()
        if (!trimmedMetadataLine) continue

        const separatorIndex = trimmedMetadataLine.indexOf(':')
        if (separatorIndex === -1) {
            throw new Error(`Markdown file ${markdownFilePath} has a frontmatter line without a value separator.`)
        }

        const key = trimmedMetadataLine.slice(0, separatorIndex).trim()
        metadataValueOfKey[key] = trimmedMetadataLine.slice(separatorIndex + 1).trim()
    }

    return {
        metadataValueOfKey,
        // HTML comments hold drafts, and this markdown is sent to the browser, so they're dropped here.
        markdown: fileText.slice(frontmatterEndIndex + FRONTMATTER_END.length).replace(/<!--[\s\S]*?-->/g, '').trim(),
    }
}

export const getRequiredMarkdownMetadataValue = ({ metadataValueOfKey, metadataKey, markdownFilePath }: { metadataValueOfKey: { [key: string]: string }; metadataKey: string; markdownFilePath: string }) => {
    const metadataValue = metadataValueOfKey[metadataKey]
    if (!metadataValue) throw new Error(`Markdown file ${markdownFilePath} needs ${metadataKey} frontmatter.`)
    return metadataValue
}

export const readMarkdownPage = async (markdownFilePath: string) => {
    const markdownFile = await readMarkdownFile(markdownFilePath)
    return {
        title: getRequiredMarkdownMetadataValue({ ...markdownFile, metadataKey: 'title', markdownFilePath }),
        description: getRequiredMarkdownMetadataValue({ ...markdownFile, metadataKey: 'description', markdownFilePath }),
        markdown: markdownFile.markdown,
    }
}
