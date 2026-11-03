import { readFileSync } from 'node:fs'
import Link from 'next/link'
import type { Metadata } from 'next'
import { DocsMarkdown } from '@/components/docs/DocsMarkdown'
import { DocsSectionsPage, type DocsSection, type DocsSectionEntry } from '@/components/docs/DocsSections'
import { DocsTypeTable, DocsTypeText } from '@/components/docs/DocsTypeTable'
import { JsonLd } from '@/components/site/JsonLd'
import { getApiTypeDocsSections, getApiValueDocs, getObjectTypeDocsPage, type ObjectTypeDoc, type TypeReferenceDoc } from '@/lib/objectTypeDocs'
import { getWebsiteFilePath } from '@/lib/markdown'
import { getBreadcrumbJsonLd } from '@/lib/siteJsonLd'
import { SITE_DEFAULT_IMAGE_ALT, SITE_DEFAULT_IMAGE_PATH, getSitePageMetadata } from '@/lib/siteMetadata'
import { DOCS_PATH } from '@/lib/siteNavigation'

export const dynamic = 'force-static'

export const metadata: Metadata = getSitePageMetadata({
    title: 'Docs',
    description: 'Our full scripting API.',
    path: DOCS_PATH,
    imageUrl: SITE_DEFAULT_IMAGE_PATH,
    imageAlt: SITE_DEFAULT_IMAGE_ALT,
})

const ScriptingApiDocsPage = () => {
    const { objectTypeDocs, typeReferenceDocs } = getObjectTypeDocsPage()
    const apiTypeDocsSections = getApiTypeDocsSections()
    const apiTypeDocs = apiTypeDocsSections.flatMap((apiTypeDocsSection) => apiTypeDocsSection.typeDocs)
    const scriptingValueDocs = getApiValueDocs(scriptingValueNames)
    const guiValueDocs = getApiValueDocs(guiValueNames)
    const objectTypeField = { field: 'type', type: objectTypeDocs.map((objectTypeDoc) => `'${objectTypeDoc.objectType}'`).join(' | '), docstring: '', isObjectIdReference: false }
    const liveObjectReferenceTypeDocs = apiTypeDocs.filter((typeDoc) => typeDoc.typeName === liveObjectTypeName).map((typeDoc) => ({ ...typeDoc, fields: typeDoc.fields.flatMap((field) => field.field === 'id' ? [field, objectTypeField] : [field]) }))
    const referenceTypeDocs = [...liveObjectReferenceTypeDocs, ...typeReferenceDocs]
    const apiTypeDocsForServices = apiTypeDocs.filter((typeDoc) => typeDoc.typeName !== liveObjectTypeName)
    const serviceTypeDocs = serviceTypeDocsOfApiTypeDocs(apiTypeDocsForServices, { GuiService: guiValueDocs })
    const objectTypeSections = objectTypeSectionsOfObjectTypeDocs(objectTypeDocs)
    const typeAnchorOfName: { [name: string]: string } = {}
    for (const objectTypeDoc of objectTypeDocs) {
        typeAnchorOfName[objectTypeDoc.tsType] = objectTypeDoc.objectType
        typeAnchorOfName[objectTypeLabelOfObjectType(objectTypeDoc.objectType)] = objectTypeDoc.objectType
    }
    for (const typeDoc of [...scriptingValueDocs, ...serviceTypeDocs.serviceAndRelatedTypeDocs, ...referenceTypeDocs]) typeAnchorOfName[typeDoc.typeName] = typeDoc.typeName
    const sections = docsSectionsOfDocs(objectTypeSections, scriptingValueDocs, referenceTypeDocs, serviceTypeDocs, typeAnchorOfName)

    return (
        <>
            <JsonLd value={getBreadcrumbJsonLd({ path: DOCS_PATH, pageName: 'Docs' })} />
            <DocsSectionsPage
                title="Docs"
                description={(
                    <>
                        <p>These are the complete docs for building games with Kube.</p>
                        <p>{`If you're a beginner, you probably don't need to read this. The chat in the editor will handle things for you.`}</p>
                        <p>
                            The first few sections define all the objects that can exist in any game.
                            The next sections discuss Scripting &amp; UI.
                            For scripting information, refer to the sections <Link href="#script" className="underline underline-offset-2">Script</Link>
                            {' '}
                            and <Link href="#functions-services" className="underline underline-offset-2">Functions &amp; Services</Link>.
                            The last section discusses Assets, Hats, and uploading files and meshes.
                        </p>
                    </>
                )}
                sections={sections}
            />
        </>
    )
}

type ObjectTypeSectionName = 'Core Objects' | 'Animation Objects' | 'Other Objects'

type ObjectTypeSection = {
    sectionName: ObjectTypeSectionName
    objectTypeDocs: ObjectTypeDoc[]
}

type ServiceTypeDocGroup = {
    serviceTypeDoc: TypeReferenceDoc
    relatedTypeDocs: TypeReferenceDoc[]
}

type ServiceTypeDocs = {
    serviceTypeDocGroups: ServiceTypeDocGroup[]
    serviceAndRelatedTypeDocs: TypeReferenceDoc[]
}

const objectTypeSectionNames: ObjectTypeSectionName[] = ['Core Objects', 'Animation Objects', 'Other Objects']
const scriptingValueNames = ['stopScript', 'tick', 'vec3']
const guiValueNames = ['useState', 'createState']
const serviceTypeNames = ['PlayerService', 'ObjectService', 'AnimationService', 'ClockService', 'CollisionService', 'ClientCharacterService', 'GuiService', 'Vec3Math']
const liveObjectTypeName = 'LiveObject'
const objectInfoTypeName = 'ObjectInfo'
// Docs display exceptions: these are useful named concepts, but their expanded unions are noisier than helpful.
const typeNamesWithoutRenderedDefinition: { [typeName: string]: true } = { [objectInfoTypeName]: true, GuiRenderable: true }

const objectTypeSectionsOfObjectTypeDocs = (objectTypeDocs: ObjectTypeDoc[]): ObjectTypeSection[] => objectTypeSectionNames.map((sectionName) => {
    const orderedObjectTypes = objectTypeOrderOfSectionName(sectionName)
    return {
        sectionName,
        objectTypeDocs: [
            ...orderedObjectTypes.flatMap((objectType) => {
                const objectTypeDoc = objectTypeDocs.find((candidateObjectTypeDoc) => candidateObjectTypeDoc.objectType === objectType)
                return objectTypeDoc === undefined ? [] : [objectTypeDoc]
            }),
            ...objectTypeDocs.filter((objectTypeDoc) => objectTypeSectionNameOfObjectType(objectTypeDoc.objectType) === sectionName && !orderedObjectTypes.includes(objectTypeDoc.objectType)),
        ],
    }
})

// The object order also controls the left navigation hierarchy.
const objectTypeOrderOfSectionName = (sectionName: ObjectTypeSectionName): string[] => {
    switch (sectionName) {
        case 'Core Objects':
            return ['ball', 'brick', 'cylinder', 'rigidGroup', 'group', 'mesh']
        case 'Animation Objects':
            return ['skAnimator', 'skeleton', 'skAnimation', 'skAttachment', 'skJoint', 'skKeyframe', 'skKeyframePose', 'propertyAnimator', 'propertyAnimation', 'propertyKeyframe']
        case 'Other Objects':
            return ['clock', 'material', 'player', 'script', 'soundPlayer', 'worldLighting']
    }
}

const objectTypeSectionNameOfObjectType = (objectType: string): ObjectTypeSectionName => {
    switch (objectType) {
        case 'skJoint':
        case 'skAttachment':
        case 'skAnimation':
        case 'skAnimator':
        case 'propertyAnimator':
        case 'propertyAnimation':
        case 'propertyKeyframe':
        case 'skKeyframe':
        case 'skKeyframePose':
        case 'skeleton':
            return 'Animation Objects'
        case 'clock':
        case 'material':
        case 'player':
        case 'script':
        case 'soundPlayer':
        case 'worldLighting':
            return 'Other Objects'
        default:
            return 'Core Objects'
    }
}

// Skeleton sub-objects sit indented under SkAnimator in the scroll spy, and the leaf keyframe/joint types are muted.
const isScrollSpyIndentedObjectType = (objectType: string): boolean => {
    switch (objectType) {
        case 'skeleton':
        case 'skAnimation':
        case 'skAttachment':
        case 'skJoint':
        case 'skKeyframe':
        case 'skKeyframePose':
        case 'propertyAnimation':
        case 'propertyKeyframe':
            return true
        default:
            return false
    }
}

const isScrollSpyMutedObjectType = (objectType: string): boolean => {
    switch (objectType) {
        case 'skJoint':
        case 'skKeyframe':
        case 'skKeyframePose':
        case 'propertyKeyframe':
            return true
        default:
            return false
    }
}

const serviceTypeDocsOfApiTypeDocs = (apiTypeDocs: TypeReferenceDoc[], extraRelatedTypeDocsOfServiceName: { [serviceTypeName: string]: TypeReferenceDoc[] }): ServiceTypeDocs => {
    const serviceTypeDocs = serviceTypeNames.flatMap((serviceTypeName) => {
        const serviceTypeDoc = apiTypeDocs.find((typeDoc) => typeDoc.typeName === serviceTypeName)
        if (serviceTypeDoc === undefined) return []
        return [serviceTypeDoc]
    })
    const didUseInScrollSpy: { [typeName: string]: true } = {}
    const serviceTypeDocGroups = serviceTypeDocs.map((serviceTypeDoc) => {
        didUseInScrollSpy[serviceTypeDoc.typeName] = true
        const relatedTypeDocs: TypeReferenceDoc[] = extraRelatedTypeDocsOfServiceName[serviceTypeDoc.typeName] ?? []
        for (const relatedTypeDoc of relatedTypeDocs) didUseInScrollSpy[relatedTypeDoc.typeName] = true
        let didAddRelatedTypeDoc = true
        while (didAddRelatedTypeDoc) {
            didAddRelatedTypeDoc = false
            for (const typeDoc of apiTypeDocs) {
                if (didUseInScrollSpy[typeDoc.typeName] === true) continue
                if (![serviceTypeDoc, ...relatedTypeDocs].some((candidateTypeDoc) => typeDocDirectlyUsesTypeName(candidateTypeDoc, typeDoc.typeName))) continue
                didUseInScrollSpy[typeDoc.typeName] = true
                relatedTypeDocs.push(typeDoc)
                didAddRelatedTypeDoc = true
            }
        }
        return { serviceTypeDoc, relatedTypeDocs }
    })
    return {
        serviceTypeDocGroups,
        serviceAndRelatedTypeDocs: serviceTypeDocGroups.flatMap((serviceTypeDocGroup) => [serviceTypeDocGroup.serviceTypeDoc, ...serviceTypeDocGroup.relatedTypeDocs]),
    }
}

const typeDocDirectlyUsesTypeName = (typeDoc: TypeReferenceDoc, typeName: string): boolean => {
    const typeNamePattern = new RegExp(`\\b${typeName}\\b`)
    if (typeNamePattern.test(typeDoc.type)) return true
    for (const field of typeDoc.fields) {
        if (typeNamePattern.test(field.type)) return true
    }
    return false
}

const docsSectionsOfDocs = (objectTypeSections: ObjectTypeSection[], scriptingValueDocs: TypeReferenceDoc[], referenceTypeDocs: TypeReferenceDoc[], serviceTypeDocs: ServiceTypeDocs, typeAnchorOfName: { [name: string]: string }): DocsSection[] => [
    ...objectTypeSections.map((objectTypeSection): DocsSection => ({
        name: objectTypeSection.sectionName,
        isMuted: false,
        entryHeadingSize: 'large',
        content: undefined,
        entries: objectTypeSection.objectTypeDocs.map((objectTypeDoc) => ({
            name: objectTypeLabelOfObjectType(objectTypeDoc.objectType),
            anchorId: objectTypeDoc.objectType,
            isIndented: isScrollSpyIndentedObjectType(objectTypeDoc.objectType),
            isMuted: isScrollSpyMutedObjectType(objectTypeDoc.objectType),
            content: (
                <>
                    {objectTypeDoc.docstring && (
                        <div className="mt-3 text-sm leading-6">
                            <DocsMarkdown text={objectTypeDoc.docstring} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
                        </div>
                    )}
                    {objectTypeDoc.fields.length > 0 && <DocsTypeTable fields={objectTypeDoc.fields} typeAnchorOfName={typeAnchorOfName} isMuted={false} />}
                </>
            ),
        })),
    })),
    {
        name: 'Functions & Services',
        isMuted: false,
        entryHeadingSize: 'small',
        content: undefined,
        entries: [
            ...scriptingValueDocs.map((typeDoc) => typeDocSectionEntry(typeDoc, false, false, typeAnchorOfName)),
            ...serviceTypeDocs.serviceTypeDocGroups.flatMap((serviceTypeDocGroup) => [
                typeDocSectionEntry(serviceTypeDocGroup.serviceTypeDoc, false, false, typeAnchorOfName),
                ...serviceTypeDocGroup.relatedTypeDocs.map((typeDoc) => typeDocSectionEntry(typeDoc, true, true, typeAnchorOfName)),
            ]),
        ],
    },
    ...writtenSections.map((writtenSection): DocsSection => ({
        name: writtenSection.name,
        isMuted: false,
        entryHeadingSize: 'small',
        content: (
            <div className="mt-3 text-sm leading-6">
                <DocsMarkdown text={writtenSection.text} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
            </div>
        ),
        entries: writtenSection.entries.map((entry) => ({
            name: entry.name,
            anchorId: entry.anchorId,
            isIndented: false,
            isMuted: false,
            content: (
                <div className="mt-3 text-sm leading-6">
                    <DocsMarkdown text={entry.text} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
                </div>
            ),
        })),
    })),
    {
        name: 'Reference',
        isMuted: true,
        entryHeadingSize: 'small',
        content: undefined,
        entries: referenceTypeDocs.map((typeDoc) => typeDocSectionEntry(typeDoc, false, true, typeAnchorOfName)),
    },
]

const systemPromptUploadsAddonMarkdownFilePath = getWebsiteFilePath('..', '..', 'crates', 'shared', 'src', 'api', 'system_prompt_uploads_addon.md')

const writtenSectionsOfMarkdown = (markdown: string) => {
    const writtenSections: { name: string, text: string, entries: { name: string, anchorId: string, text: string }[] }[] = []
    for (const line of markdown.split('\n')) {
        const writtenSection = writtenSections[writtenSections.length - 1]
        if (line.startsWith('## ')) {
            writtenSections.push({ name: line.slice(3).trim(), text: '', entries: [] })
        } else if (line.startsWith('### ') && writtenSection !== undefined) {
            const name = line.slice(4).trim()
            writtenSection.entries.push({ name, anchorId: name.toLowerCase().replace(/[^a-z0-9]+/g, '-'), text: '' })
        } else if (writtenSection !== undefined) {
            const entry = writtenSection.entries[writtenSection.entries.length - 1]
            if (entry === undefined) writtenSection.text += `${line}\n`
            else entry.text += `${line}\n`
        }
    }
    return writtenSections
}

const writtenSections = writtenSectionsOfMarkdown(readFileSync(systemPromptUploadsAddonMarkdownFilePath, 'utf8'))

const typeDocSectionEntry = (typeDoc: TypeReferenceDoc, isIndented: boolean, isMuted: boolean, typeAnchorOfName: { [name: string]: string }): DocsSectionEntry => ({
    name: typeDoc.typeName,
    anchorId: typeDoc.typeName,
    isIndented,
    isMuted,
    content: (
        <>
            {typeDoc.docstring && (
                <div className="mt-3 text-sm leading-6">
                    <DocsMarkdown text={typeDoc.docstring} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
                </div>
            )}
            {typeNamesWithoutRenderedDefinition[typeDoc.typeName] ? null : typeDoc.fields.length === 0 ? (
                <div className="mt-3 font-mono text-sm text-gray-700">
                    <DocsTypeText text={typeDoc.type} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
                </div>
            ) : (
                <DocsTypeTable fields={typeDoc.fields} typeAnchorOfName={typeAnchorOfName} isMuted={false} />
            )}
        </>
    ),
})

const objectTypeLabelOfObjectType = (objectType: string): string => {
    const firstCharacter = objectType[0]
    if (firstCharacter === undefined) return objectType
    return firstCharacter.toUpperCase() + objectType.slice(1)
}

export default ScriptingApiDocsPage
