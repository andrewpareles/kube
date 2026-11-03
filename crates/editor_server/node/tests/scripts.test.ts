import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { readScriptDiagnostics, scriptTypeEnvironment } from '../src/helpers/scripts.ts'

test('script diagnostics use runtime globals and distinguish TypeScript errors', () => {
    const result = readScriptDiagnostics([
        {
            objectId: 2,
            path: 'root/main',
            text: 'const currentGame = game; const x: number = "bad";',
        },
    ])
    assert.equal(result[2].length, 1)
    assert.match(result[2][0].message, /not assignable/)
})
test('the character kit scripts type-check like any world script', () => {
    const files = ['character_controller.ts', 'character_spawn.ts', 'character_animation.ts', 'zoom_fade.ts']
    const scripts = files.map((file, index) => ({
        objectId: index,
        path: `root/CharacterController/${file}`,
        text: readFileSync(new URL(`../../../../assets/scripts/${file}`, import.meta.url), 'utf8'),
    }))
    assert.deepEqual(readScriptDiagnostics(scripts), { 0: [], 1: [], 2: [], 3: [] })
})
test('scripts can use vec3 without importing it, because the runtime injects it', () => {
    const result = readScriptDiagnostics([
        {
            objectId: 2,
            path: 'root/main',
            text: 'const up = vec3.normalize({ x: 0, y: 2, z: 0 }); const x: number = vec3.length(up);',
        },
    ])
    assert.deepEqual(result, { 2: [] })
})
test('world modules resolve each other without writing files', () => {
    const result = readScriptDiagnostics([
        {
            objectId: 2,
            path: 'root/main',
            text: 'import { count } from "./counter"; const x: number = count;',
        },
        { objectId: 3, path: 'root/counter', text: 'export const count = 2;' },
    ])
    assert.deepEqual(result, { 2: [], 3: [] })
})
test('the browser type environment includes JSX dependencies and the server checks their props', () => {
    const environment = JSON.parse(JSON.stringify(scriptTypeEnvironment))
    assert.match(environment.declarations['/world/globals.d.ts'], /const game:/)
    assert.ok(environment.declarations['/node_modules/kube-ts-api/types/gui/guiTypes.d.ts'])
    const result = readScriptDiagnostics([
        {
            objectId: 2,
            path: 'root/gui',
            text: 'const element = <div className="p-2">Hello</div>;',
        },
        {
            objectId: 3,
            path: 'root/wrong',
            text: 'const element = <div className={42}>Hello</div>;',
        },
    ])
    assert.deepEqual(result[2], [])
    assert.equal(result[3].length, 1)
    assert.match(result[3][0].message, /not assignable/)
})
test('diagnostics preserve ranges and include an empty list for clean scripts', () => {
    const result = readScriptDiagnostics([
        { objectId: 2, path: 'root/main', text: 'const x: number = "bad";' },
        { objectId: 3, path: 'root/clean', text: 'const x = 1;' },
    ])
    assert.deepEqual(Object.keys(result), ['2', '3'])
    assert.equal(result[2][0].startLineNumber, 1)
    assert.equal(result[2][0].severity, 'error')
    assert.deepEqual(result[3], [])
})

test('draft syntax and API errors disappear when corrected, without changing the supplied scripts', () => {
    const scripts = [
        { objectId: 2, path: 'root/main', text: 'const x: number = ;' },
        { objectId: 3, path: 'root/api', text: 'game.objectService.thisMethodDoesNotExist();' },
    ]
    const original = structuredClone(scripts)
    const errors = readScriptDiagnostics(scripts)
    assert.ok(errors[2].some((error) => error.message.includes('Expression expected')))
    assert.ok(errors[3].some((error) => error.message.includes('does not exist')))
    assert.deepEqual(scripts, original)
    scripts[0].text = 'const x: number = 1;'
    scripts[1].text = 'const service = game.objectService;'
    assert.deepEqual(readScriptDiagnostics(scripts), { 2: [], 3: [] })
})
