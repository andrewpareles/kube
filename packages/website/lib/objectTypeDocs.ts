import path from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

export type ObjectTypeDocField = {
    field: string
    type: string
    docstring: string
    isObjectIdReference: boolean
}

export type ObjectTypeDoc = {
    objectType: string
    tsType: string
    docstring: string
    fields: ObjectTypeDocField[]
}

export type TypeReferenceDoc = {
    typeName: string
    type: string
    docstring: string
    fields: ObjectTypeDocField[]
}

export type ObjectTypeDocsPage = {
    objectTypeDocs: ObjectTypeDoc[]
    typeReferenceDocs: TypeReferenceDoc[]
}

export type ApiTypeDocsSection = {
    sectionName: string
    typeDocs: TypeReferenceDoc[]
}

const websiteRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const kubeTsApiSrcRoot = path.join(websiteRoot, '../kube-ts-api/src')
const indexPublicPath = path.join(kubeTsApiSrcRoot, 'index-public.ts')
const objectTypesPath = path.join(kubeTsApiSrcRoot, 'object_types/types.ts')
const sharedTypesPath = path.join(kubeTsApiSrcRoot, 'object_types/shared.ts')
const apiTypeSectionOrder = ['World', 'Player Service', 'Object Service', 'Template Service', 'Animation Service', 'Clock Service', 'Collision Service', 'GUI Types', 'GUI Service']

export const getObjectTypeDocs = (): ObjectTypeDoc[] => getObjectTypeDocsPage().objectTypeDocs

export const getObjectTypeDocsPage = (): ObjectTypeDocsPage => {
    const program = ts.createProgram([objectTypesPath, sharedTypesPath], {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        strict: true,
        skipLibCheck: true,
    })
    const checker = program.getTypeChecker()
    const objectTypesSourceFile = program.getSourceFile(objectTypesPath)
    const sharedTypesSourceFile = program.getSourceFile(sharedTypesPath)
    if (objectTypesSourceFile === undefined || sharedTypesSourceFile === undefined) throw new Error('Object type source files were not found.')

    const typeAliasOfName: { [name: string]: ts.TypeAliasDeclaration } = {}
    const typeAliases: ts.TypeAliasDeclaration[] = []
    for (const sourceFile of [objectTypesSourceFile, sharedTypesSourceFile]) {
        ts.forEachChild(sourceFile, (node) => {
            if (!ts.isTypeAliasDeclaration(node)) return
            typeAliasOfName[node.name.text] = node
            typeAliases.push(node)
        })
    }

    const objectInfoAlias = typeAliasOfName.ObjectInfo
    if (objectInfoAlias === undefined || !ts.isUnionTypeNode(unwrapTypeNode(objectInfoAlias.type))) throw new Error('ObjectInfo must be a union type.')

    const objectInfoType = unwrapTypeNode(objectInfoAlias.type)
    if (!ts.isUnionTypeNode(objectInfoType)) throw new Error('ObjectInfo must be a union type.')

    const getObjectTypeParts = (typeNode: ts.TypeNode): { objectType: string; tsType: string } => {
        let objectType = ''
        let tsType = ''

        const visitNode = (node: ts.Node) => {
            if (ts.isTypeLiteralNode(node)) {
                for (const member of node.members) {
                    if (!ts.isPropertySignature(member) || member.type === undefined) continue
                    if (getFieldName(member.name) !== 'type' || !ts.isLiteralTypeNode(member.type) || !ts.isStringLiteral(member.type.literal)) continue
                    objectType = member.type.literal.text
                }
            }

            if (ts.isTypeReferenceNode(node) && ts.isIdentifier(node.typeName) && node.typeName.text.endsWith('Info') && node.typeName.text !== 'ObjectInfo') {
                tsType = node.typeName.text
            }

            ts.forEachChild(node, visitNode)
        }

        visitNode(typeNode)
        if (!objectType || !tsType) throw new Error('ObjectInfo member must include an object type and an Info type.')
        return { objectType, tsType }
    }

    const objectTypeDocs = objectInfoType.types.map((typeNode) => {
        const objectTypeParts = getObjectTypeParts(typeNode)
        const typeAlias = typeAliasOfName[objectTypeParts.tsType]
        if (typeAlias === undefined) throw new Error(`Missing type alias ${objectTypeParts.tsType}.`)

        const fields: ObjectTypeDocField[] = []
        const didReadFieldName: { [fieldName: string]: true } = {}
        collectFields(typeAlias.type, typeAliasOfName, checker, fields, didReadFieldName)

        return {
            ...objectTypeParts,
            docstring: docstringOfTypeAlias(typeAlias, checker),
            fields,
        }
    }).sort((leftObjectTypeDoc, rightObjectTypeDoc) => leftObjectTypeDoc.objectType.localeCompare(rightObjectTypeDoc.objectType))

    const isObjectInfoTypeName: { [typeName: string]: true } = {}
    for (const objectTypeDoc of objectTypeDocs) isObjectInfoTypeName[objectTypeDoc.tsType] = true

    const typeReferenceDocs = typeAliases.flatMap((typeAlias) => {
        const typeName = typeAlias.name.text
        if (typeName === 'RustObjectSnapshot' || isObjectInfoTypeName[typeName]) return []

        return [typeReferenceDocOfTypeAlias(typeAlias, typeAliasOfName, checker)]
    })

    return {
        objectTypeDocs,
        typeReferenceDocs,
    }
}

export const getApiTypeDocsSections = (): ApiTypeDocsSection[] => {
    const program = ts.createProgram([indexPublicPath, objectTypesPath, sharedTypesPath], {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        strict: true,
        skipLibCheck: true,
    })
    const checker = program.getTypeChecker()
    const typeAliasOfName: { [name: string]: ts.TypeAliasDeclaration } = {}
    for (const sourceFile of program.getSourceFiles()) {
        if (sourceFile.isDeclarationFile || !sourceFile.fileName.includes('/packages/kube-ts-api/src/')) continue
        ts.forEachChild(sourceFile, (node) => {
            if (!ts.isTypeAliasDeclaration(node)) return
            typeAliasOfName[node.name.text] = node
        })
    }

    const indexPublicSourceFile = program.getSourceFile(indexPublicPath)
    if (indexPublicSourceFile === undefined) throw new Error('Public index source file was not found.')

    const indexPublicSymbol = checker.getSymbolAtLocation(indexPublicSourceFile)
    if (indexPublicSymbol === undefined) throw new Error('Public index source file has no module symbol.')

    const objectTypeDocsPage = getObjectTypeDocsPage()
    const isAlreadyShownTypeName: { [typeName: string]: true } = { RustObjectSnapshot: true }
    for (const objectTypeDoc of objectTypeDocsPage.objectTypeDocs) isAlreadyShownTypeName[objectTypeDoc.tsType] = true
    for (const typeReferenceDoc of objectTypeDocsPage.typeReferenceDocs) isAlreadyShownTypeName[typeReferenceDoc.typeName] = true

    const typeDocsOfSectionName: { [sectionName: string]: TypeReferenceDoc[] } = {}
    const didReadTypeName: { [typeName: string]: true } = {}
    for (const exportedSymbol of checker.getExportsOfModule(indexPublicSymbol)) {
        const symbol = (exportedSymbol.flags & ts.SymbolFlags.Alias) === 0 ? exportedSymbol : checker.getAliasedSymbol(exportedSymbol)
        const typeAlias = symbol.getDeclarations()?.find((declaration) => ts.isTypeAliasDeclaration(declaration))
        if (typeAlias === undefined) continue

        const typeName = typeAlias.name.text
        if (didReadTypeName[typeName] || isAlreadyShownTypeName[typeName]) continue
        didReadTypeName[typeName] = true

        const sectionName = apiTypeSectionNameOfSourcePath(typeAlias.getSourceFile().fileName)
        const typeDocs = typeDocsOfSectionName[sectionName] ?? []
        typeDocs.push(typeReferenceDocOfTypeAlias(typeAlias, typeAliasOfName, checker))
        typeDocsOfSectionName[sectionName] = typeDocs
    }

    const sections = apiTypeSectionOrder.flatMap((sectionName) => {
        const typeDocs = typeDocsOfSectionName[sectionName]
        if (typeDocs === undefined || typeDocs.length === 0) return []
        return [{ sectionName, typeDocs }]
    })
    for (const sectionName of Object.keys(typeDocsOfSectionName)) {
        if (apiTypeSectionOrder.includes(sectionName)) continue
        sections.push({ sectionName, typeDocs: typeDocsOfSectionName[sectionName] })
    }
    return sections
}

export const getApiValueDocs = (valueNames: string[]): TypeReferenceDoc[] => {
    const program = ts.createProgram([indexPublicPath, objectTypesPath, sharedTypesPath], {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        strict: true,
        skipLibCheck: true,
    })
    const checker = program.getTypeChecker()
    const indexPublicSourceFile = program.getSourceFile(indexPublicPath)
    if (indexPublicSourceFile === undefined) throw new Error('Public index source file was not found.')

    const indexPublicSymbol = checker.getSymbolAtLocation(indexPublicSourceFile)
    if (indexPublicSymbol === undefined) throw new Error('Public index source file has no module symbol.')

    const exportedSymbolOfName: { [valueName: string]: ts.Symbol } = {}
    for (const exportedSymbol of checker.getExportsOfModule(indexPublicSymbol)) exportedSymbolOfName[exportedSymbol.name] = exportedSymbol

    return valueNames.flatMap((valueName) => {
        const exportedSymbol = exportedSymbolOfName[valueName]
        if (exportedSymbol === undefined) return []

        const symbol = (exportedSymbol.flags & ts.SymbolFlags.Alias) === 0 ? exportedSymbol : checker.getAliasedSymbol(exportedSymbol)
        const declaration = symbol.valueDeclaration ?? symbol.declarations?.[0]
        if (declaration === undefined) return []

        return [{
            typeName: valueName,
            type: checker.typeToString(checker.getTypeOfSymbolAtLocation(symbol, declaration), declaration, ts.TypeFormatFlags.NoTruncation),
            docstring: ts.displayPartsToString(symbol.getDocumentationComment(checker)).trim(),
            fields: [],
        }]
    })
}

const unwrapTypeNode = (typeNode: ts.TypeNode): ts.TypeNode => {
    let unwrappedTypeNode = typeNode
    while (ts.isParenthesizedTypeNode(unwrappedTypeNode)) unwrappedTypeNode = unwrappedTypeNode.type
    return unwrappedTypeNode
}

const getFieldName = (propertyName: ts.PropertyName): string => {
    if (ts.isIdentifier(propertyName) || ts.isStringLiteral(propertyName) || ts.isNumericLiteral(propertyName)) return propertyName.text
    throw new Error('Unsupported property name in object type docs.')
}

const docstringOfTypeAlias = (typeAlias: ts.TypeAliasDeclaration, checker: ts.TypeChecker): string => {
    const symbol = checker.getSymbolAtLocation(typeAlias.name)
    return symbol === undefined ? '' : ts.displayPartsToString(symbol.getDocumentationComment(checker)).trim()
}

const typeReferenceDocOfTypeAlias = (typeAlias: ts.TypeAliasDeclaration, typeAliasOfName: { [name: string]: ts.TypeAliasDeclaration }, checker: ts.TypeChecker): TypeReferenceDoc => {
    const fields: ObjectTypeDocField[] = []
    const didReadFieldName: { [fieldName: string]: true } = {}
    collectFields(typeAlias.type, typeAliasOfName, checker, fields, didReadFieldName)

    return {
        typeName: typeAlias.name.text,
        type: typeAlias.type.getText(typeAlias.getSourceFile()).replace(/\s+/g, ' '),
        docstring: docstringOfTypeAlias(typeAlias, checker),
        fields,
    }
}

const collectFields = (
    typeNode: ts.TypeNode,
    typeAliasOfName: { [name: string]: ts.TypeAliasDeclaration },
    checker: ts.TypeChecker,
    fields: ObjectTypeDocField[],
    didReadFieldName: { [fieldName: string]: true },
) => {
    const unwrappedTypeNode = unwrapTypeNode(typeNode)

    if (ts.isIntersectionTypeNode(unwrappedTypeNode)) {
        for (const childTypeNode of unwrappedTypeNode.types) collectFields(childTypeNode, typeAliasOfName, checker, fields, didReadFieldName)
        return
    }

    if (ts.isTypeReferenceNode(unwrappedTypeNode) && ts.isIdentifier(unwrappedTypeNode.typeName)) {
        const childTypeAlias = typeAliasOfName[unwrappedTypeNode.typeName.text]
        if (childTypeAlias !== undefined) collectFields(childTypeAlias.type, typeAliasOfName, checker, fields, didReadFieldName)
        return
    }

    if (!ts.isTypeLiteralNode(unwrappedTypeNode)) return

    for (const member of unwrappedTypeNode.members) {
        if (!ts.isPropertySignature(member) || member.type === undefined) continue
        const field = getFieldName(member.name)
        if (didReadFieldName[field]) continue
        didReadFieldName[field] = true

        const symbol = checker.getSymbolAtLocation(member.name)
        const isObjectIdReference = typeNodeContainsObjectId(member.type, typeAliasOfName, {})
        const docstring = symbol === undefined ? '' : ts.displayPartsToString(symbol.getDocumentationComment(checker)).trim()
        fields.push({
            field,
            type: member.type.getText(member.getSourceFile()).replace(/\s+/g, ' '),
            docstring,
            isObjectIdReference,
        })
    }
}

const apiTypeSectionNameOfSourcePath = (sourcePath: string): string => {
    switch (path.relative(kubeTsApiSrcRoot, sourcePath)) {
        case 'world.ts':
            return 'World'
        case 'playerService.ts':
            return 'Player Service'
        case 'objectService.ts':
            return 'Object Service'
        case 'templateService.ts':
            return 'Template Service'
        case 'animationService.ts':
            return 'Animation Service'
        case 'clockService.ts':
            return 'Clock Service'
        case 'collisionService.ts':
            return 'Collision Service'
        case path.join('gui', 'guiTypes.ts'):
            return 'GUI Types'
        case path.join('gui', 'guiService.ts'):
            return 'GUI Service'
        default:
            return path.basename(sourcePath, '.ts')
    }
}

const typeNodeContainsObjectId = (
    typeNode: ts.TypeNode,
    typeAliasOfName: { [name: string]: ts.TypeAliasDeclaration },
    didReadTypeName: { [typeName: string]: true },
): boolean => {
    const unwrappedTypeNode = unwrapTypeNode(typeNode)
    if (ts.isTypeReferenceNode(unwrappedTypeNode) && ts.isIdentifier(unwrappedTypeNode.typeName)) {
        const typeName = unwrappedTypeNode.typeName.text
        if (typeName === 'ObjectId') return true
        if (didReadTypeName[typeName]) return false
        didReadTypeName[typeName] = true

        const typeAlias = typeAliasOfName[typeName]
        return typeAlias === undefined ? false : typeNodeContainsObjectId(typeAlias.type, typeAliasOfName, didReadTypeName)
    }

    if (ts.isUnionTypeNode(unwrappedTypeNode) || ts.isIntersectionTypeNode(unwrappedTypeNode)) {
        for (const childTypeNode of unwrappedTypeNode.types) {
            if (typeNodeContainsObjectId(childTypeNode, typeAliasOfName, didReadTypeName)) return true
        }
    }

    if (ts.isTypeLiteralNode(unwrappedTypeNode)) {
        for (const member of unwrappedTypeNode.members) {
            if (ts.isPropertySignature(member) && member.type !== undefined && typeNodeContainsObjectId(member.type, typeAliasOfName, didReadTypeName)) return true
        }
    }

    return false
}
