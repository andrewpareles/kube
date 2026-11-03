import assert from 'node:assert/strict'
import test from 'node:test'
import { beginEditorPointer, moveEditorPointer, finishEditorPointer, type EditorPointer } from '../packages/website/app/editor/editorPointer.ts'

test('clicks tolerate hand movement without moving the camera', () => {
    const pointers: { [id: number]: EditorPointer } = {}
    beginEditorPointer(pointers, 1, 20, 30, 0)
    assert.deepEqual(moveEditorPointer(pointers[1], 22, 31), { dx: 0, dy: 0 })
    assert.deepEqual(finishEditorPointer(pointers, 1, 22, 31), { x: 22, y: 31 })
    assert.deepEqual(pointers, {})
})

test('dragging includes the initial movement and cannot become a click by returning to the start', () => {
    const pointers: { [id: number]: EditorPointer } = {}
    beginEditorPointer(pointers, 1, 20, 30, 0)
    moveEditorPointer(pointers[1], 22, 30)
    assert.deepEqual(moveEditorPointer(pointers[1], 30, 30), { dx: 10, dy: 0 })
    assert.deepEqual(moveEditorPointer(pointers[1], 20, 30), { dx: -10, dy: 0 })
    assert.equal(finishEditorPointer(pointers, 1, 20, 30), null)
})

test('releasing beyond the threshold without a move event still cancels selection', () => {
    const pointers: { [id: number]: EditorPointer } = {}
    beginEditorPointer(pointers, 1, 0, 0, 0)
    assert.equal(finishEditorPointer(pointers, 1, 100, 100), null)
})

test('right clicks and canceled gestures never select an object', () => {
    const pointers: { [id: number]: EditorPointer } = {}
    beginEditorPointer(pointers, 1, 10, 10, 2)
    assert.equal(finishEditorPointer(pointers, 1, 10, 10), null)
    beginEditorPointer(pointers, 1, 10, 10, 0)
    delete pointers[1]
    assert.equal(finishEditorPointer(pointers, 1, 10, 10), null)
})

test('both fingers in a pinch are excluded from selection, including the last finger released', () => {
    const pointers: { [id: number]: EditorPointer } = {}
    beginEditorPointer(pointers, 1, 0, 0, 0)
    beginEditorPointer(pointers, 2, 10, 10, 0)
    moveEditorPointer(pointers[1], 20, 20)
    assert.equal(finishEditorPointer(pointers, 2, 10, 10), null)
    assert.deepEqual(moveEditorPointer(pointers[1], 21, 20), { dx: 1, dy: 0 })
    assert.equal(finishEditorPointer(pointers, 1, 21, 20), null)
    beginEditorPointer(pointers, 3, 5, 5, 0)
    assert.deepEqual(finishEditorPointer(pointers, 3, 5, 5), { x: 5, y: 5 })
})
