import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { getEditorHistoryDirection, getEditorSelectionShortcut } from '../packages/website/app/editor/editorHistory.ts'

// Undo and redo themselves belong to the editor server. All the browser decides is whether a
// keystroke is one of them, and whether the control under it wants the keystroke for itself.
test('undo and redo shortcuts leave text controls to their own undo', () => {
    const { JSDOM } = createRequire(new URL('../packages/website/package.json', import.meta.url))('jsdom')
    const dom = new JSDOM('<!doctype html><html><body><input id="text" /><textarea id="chat"></textarea><div role="alertdialog"><button id="cancel"></button></div><button id="dropdown" role="combobox"></button></body></html>')
    Object.assign(globalThis, { HTMLElement: dom.window.HTMLElement })
    const press = (element: HTMLElement, options: KeyboardEventInit) => {
        const directions: ('undo' | 'redo' | null)[] = []
        element.addEventListener('keydown', event => directions.push(getEditorHistoryDirection(event)), { once: true })
        element.dispatchEvent(new dom.window.KeyboardEvent('keydown', options))
        return directions[0]
    }
    const { body } = dom.window.document
    const textField: HTMLElement = dom.window.document.getElementById('text')
    const chatBox: HTMLElement = dom.window.document.getElementById('chat')
    assert.equal(press(body, { key: 'z', metaKey: true }), 'undo')
    assert.equal(press(body, { key: 'z', ctrlKey: true }), 'undo')
    assert.equal(press(body, { key: 'Z', metaKey: true, shiftKey: true }), 'redo')
    assert.equal(press(body, { key: 'y', metaKey: true }), 'redo')
    assert.equal(press(body, { key: 'z' }), null, 'Plain typing is not a shortcut')
    assert.equal(press(body, { key: 'z', metaKey: true, altKey: true }), null)
    assert.equal(press(textField, { key: 'z', metaKey: true }), null, 'A focused property field undoes its own text')
    assert.equal(press(chatBox, { key: 'z', metaKey: true }), null, 'The chat box undoes its own text')
    assert.equal(press(dom.window.document.getElementById('cancel'), { key: 'z', metaKey: true }), null, 'A modal does not undo the world behind it')
    assert.equal(press(dom.window.document.getElementById('dropdown'), { key: 'z', metaKey: true }), 'undo', 'A property dropdown still undoes its change')
})

test('F frames and Command or Control A selects the root, except inside controls that want the key', () => {
    const { JSDOM } = createRequire(new URL('../packages/website/package.json', import.meta.url))('jsdom')
    const dom = new JSDOM('<!doctype html><html><body><input id="text" /><textarea id="chat"></textarea><div class="monaco-editor"><div id="code" tabindex="0"></div></div><button id="type" role="combobox"></button></body></html>')
    Object.assign(globalThis, { HTMLElement: dom.window.HTMLElement })
    const press = (element: HTMLElement, options: KeyboardEventInit) => {
        const shortcuts: ('select-root' | 'frame' | null)[] = []
        element.addEventListener('keydown', event => shortcuts.push(getEditorSelectionShortcut(event)), { once: true })
        element.dispatchEvent(new dom.window.KeyboardEvent('keydown', options))
        return shortcuts[0]
    }
    const { body } = dom.window.document
    const element = (id: string): HTMLElement => dom.window.document.getElementById(id)
    assert.equal(press(body, { key: 'f' }), 'frame')
    assert.equal(press(body, { key: 'f', repeat: true }), null, 'Holding F frames once')
    assert.equal(press(body, { key: 'f', metaKey: true }), null)
    assert.equal(press(element('chat'), { key: 'f' }), null, 'The chat box types an f')
    assert.equal(press(element('text'), { key: 'f' }), null, 'A property field types an f')
    assert.equal(press(element('code'), { key: 'f' }), null, 'Monaco focuses a div that is not contentEditable')
    assert.equal(press(element('type'), { key: 'f' }), null, 'A focused select picks the option starting with f')
    assert.equal(press(body, { key: 'a', metaKey: true }), 'select-root')
    assert.equal(press(body, { key: 'a', ctrlKey: true }), 'select-root')
    assert.equal(press(element('text'), { key: 'a', metaKey: true }), null, 'A property field selects its own text')
    assert.equal(press(body, { key: 'A', metaKey: true, shiftKey: true }), null)
})
