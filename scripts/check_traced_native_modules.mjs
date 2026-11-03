import { readdir, readFile, stat } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const websitePath = path.join(path.dirname(fileURLToPath(import.meta.url)), '../packages/website')
const modulesPath = path.join(websitePath, 'node_modules')
const serverPath = path.join(websitePath, '.next/server')
const isNativeBinary = name => /\.node$|\.dylib$|\.so(\.\d+)*$/.test(name)

const walk = async (directoryPath, onFile) => {
    let entries
    try { entries = await readdir(directoryPath, { withFileTypes: true }) } catch { return }
    for (const entry of entries) {
        if (entry.name === '.cache' || entry.name === '.bin') continue
        const entryPath = path.join(directoryPath, entry.name)
        if (entry.isDirectory()) await walk(entryPath, onFile)
        else if (entry.isFile()) onFile(entryPath)
    }
}

// The package that owns a file is the one under its innermost node_modules, so that a nested
// copy is not attributed to the dependency that vendored it.
const owningPackage = filePath => {
    const parts = filePath.split(path.sep)
    const index = parts.lastIndexOf('node_modules')
    if (index === -1 || index + 1 >= parts.length) return undefined
    const rest = parts.slice(index + 1)
    const name = rest[0].startsWith('@') ? rest.slice(0, 2).join('/') : rest[0]
    return `${parts.slice(0, index + 1).join(path.sep)}${path.sep}${name}`
}

const packages = new Map()
await walk(modulesPath, filePath => {
    const key = owningPackage(filePath)
    if (key === undefined) return
    const entry = packages.get(key) ?? { native: new Set(), other: new Set() }
    if (isNativeBinary(path.basename(filePath))) entry.native.add(filePath)
    else entry.other.add(filePath)
    packages.set(key, entry)
})
const nativePackages = [...packages].filter(([, entry]) => entry.native.size > 0)

const traceFilePaths = []
await walk(serverPath, filePath => { if (filePath.endsWith('.nft.json')) traceFilePaths.push(filePath) })
if (traceFilePaths.length === 0) throw new Error(`No .nft.json traces under ${serverPath}; run next build first`)

const failures = []
for (const traceFilePath of traceFilePaths) {
    const base = path.dirname(traceFilePath)
    const { files } = JSON.parse(await readFile(traceFilePath, 'utf8'))
    const traced = new Set(files.map(file => path.resolve(base, file)))
    const route = path.relative(serverPath, traceFilePath).replace(/\.js\.nft\.json$/, '')
    for (const [key, entry] of nativePackages) {
        // Tracing pulled in this package's JavaScript, so the route can reach it at run time.
        if (![...entry.other].some(file => traced.has(file))) continue
        const missing = [...entry.native].filter(file => !traced.has(file))
        if (missing.length > 0) failures.push({ route, name: path.relative(modulesPath, key), missing })
    }
}

if (failures.length > 0) {
    console.error('Native modules reachable at run time but missing from the traced output:\n')
    for (const { route, name, missing } of failures) {
        console.error(`  ${route}`)
        console.error(`    ${name}`)
        for (const file of missing) console.error(`      ${path.relative(modulesPath, file)}`)
    }
    console.error('\nAdd the package to outputFileTracingIncludes in next.config.ts for the affected route.')
    process.exit(1)
}
console.log(`Checked ${traceFilePaths.length} traces against ${nativePackages.length} native packages; all binaries present.`)
