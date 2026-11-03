import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDirectoryPath = path.dirname(fileURLToPath(import.meta.url))
const repoRootPath = path.resolve(scriptDirectoryPath, '..')
const cargoToml = fs.readFileSync(path.join(repoRootPath, 'Cargo.toml'), 'utf8')

const workspacePackageMatch = cargoToml.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/)
if (!workspacePackageMatch) {
    throw new Error('Failed to find [workspace.package] in Cargo.toml')
}

const releaseVersionMatch = workspacePackageMatch[1].match(/^\s*version\s*=\s*"([^"]+)"\s*$/m)
if (!releaseVersionMatch) {
    throw new Error('Failed to find workspace package version in Cargo.toml')
}

process.stdout.write(releaseVersionMatch[1])
