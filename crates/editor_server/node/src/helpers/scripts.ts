import ts from 'typescript'
import { readFileSync, readdirSync } from 'node:fs'
import { z } from 'zod'

const declarationsFile = new URL(
    '../../../../../packages/kube-ts-api/dist/kube-ts-api.api.d.ts',
    import.meta.url,
)
const typesDirectory = new URL('../../../../../packages/kube-ts-api/dist/types/', import.meta.url)
const declarationsPath = '/node_modules/kube-ts-api/kube-ts-api.api.d.ts'
const jsxDeclarationsPath = '/node_modules/kube-ts-api/types/jsx-runtime.d.ts'
// These globals and compiler options match the original Monaco script workspace.
const globals = `declare global {
  const createState: typeof import("kube-ts-api").createState;
  const game: typeof import("kube-ts-api").game;
  const getDelayedWorldTime: typeof import("kube-ts-api").getDelayedWorldTime;
  const getTime: typeof import("kube-ts-api").getTime;
  const script: typeof import("kube-ts-api").script;
  const stopScript: typeof import("kube-ts-api").stopScript;
  const tick: typeof import("kube-ts-api").tick;
  const useState: typeof import("kube-ts-api").useState;
  const vec3: typeof import("kube-ts-api").vec3;
}
export {};`

// Both checkers use this virtual filesystem and these compiler options, including JSX's imported types.
const declarations: { [path: string]: string } = {
    [declarationsPath]: readFileSync(declarationsFile, 'utf8'),
    '/world/globals.d.ts': globals,
}
for (const file of readdirSync(typesDirectory, { recursive: true, encoding: 'utf8' })) {
    if (file.endsWith('.d.ts'))
        declarations[`/node_modules/kube-ts-api/types/${file}`] = readFileSync(
            new URL(file, typesDirectory),
            'utf8',
        )
}
export const scriptTypeEnvironment = {
    declarations,
    compilerOptions: {
        noEmit: true,
        strict: false,
        noImplicitAny: false,
        moduleDetection: ts.ModuleDetectionKind.Force,
        jsx: ts.JsxEmit.ReactJSX,
        jsxImportSource: 'kube-ts-api',
        paths: {
            'kube-ts-api': [declarationsPath],
            'kube-ts-api/jsx-runtime': [jsxDeclarationsPath],
        },
        target: ts.ScriptTarget.ESNext,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        skipLibCheck: true,
    } satisfies ts.CompilerOptions,
}
export const scriptSnapshotSchema = z.array(
    z.object({ objectId: z.number(), path: z.string(), text: z.string() }),
)

// TypeScript checks the actual world scripts and the same API declarations shipped to the model.
export const readScriptDiagnostics = (scripts: z.infer<typeof scriptSnapshotSchema>) => {
    const textOfPath: { [path: string]: string | undefined } = { ...declarations }
    for (const script of scripts) textOfPath[`/world/${script.path}.tsx`] = script.text
    const options = scriptTypeEnvironment.compilerOptions
    const host = ts.createCompilerHost(options)
    const readFile = host.readFile
    const fileExists = host.fileExists
    host.readFile = (path) => textOfPath[path] ?? readFile(path)
    const directoryExists = host.directoryExists
    host.directoryExists = (path) =>
        Object.keys(textOfPath).some((file) => file.startsWith(`${path}/`)) ||
        directoryExists?.(path) === true
    host.fileExists = (path) => textOfPath[path] !== undefined || fileExists(path)
    host.getSourceFile = (path, languageVersion) => {
        const text = host.readFile(path)
        return text === undefined ? undefined : ts.createSourceFile(path, text, languageVersion)
    }
    const program = ts.createProgram(Object.keys(textOfPath), options, host)
    const diagnostics = ts.getPreEmitDiagnostics(program)
    return Object.fromEntries(
        scripts.map((script) => {
            const errors = diagnostics
                .filter((diagnostic) => diagnostic.file?.fileName === `/world/${script.path}.tsx`)
                .map((diagnostic) => {
                    const location = diagnostic.file?.getLineAndCharacterOfPosition(
                        diagnostic.start ?? 0,
                    )
                    const end = diagnostic.file?.getLineAndCharacterOfPosition(
                        (diagnostic.start ?? 0) + (diagnostic.length ?? 0),
                    )
                    return {
                        message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
                        startLineNumber: (location?.line ?? 0) + 1,
                        startColumn: (location?.character ?? 0) + 1,
                        endLineNumber: (end?.line ?? 0) + 1,
                        endColumn: (end?.character ?? 0) + 1,
                        severity:
                            diagnostic.category === ts.DiagnosticCategory.Error
                                ? 'error'
                                : 'warning',
                    }
                })
            return [script.objectId, errors] satisfies [number, typeof errors]
        }),
    )
}
