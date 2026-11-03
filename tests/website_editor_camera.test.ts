import assert from 'node:assert/strict'
import test from 'node:test'
import { getEditorCameraOfLocation, getEditorCameraTarget, getLocationOfEditorCamera } from '../packages/website/app/editor/editorCamera.ts'

test('a saved camera and the editor camera look the same way, and convert back unchanged', () => {
    const location = { position: [3, 4, 5] as [number, number, number], orientation: { theta: 1.2, phi: 0.4 } }
    const camera = getEditorCameraOfLocation(location)
    assert.ok(camera)
    // Rust's Orientation3D::to_unit_direction.
    const direction = [Math.cos(0.4) * Math.sin(1.2), -Math.sin(0.4), Math.cos(0.4) * Math.cos(1.2)]
    const target = getEditorCameraTarget(camera)
    for (const axis of [0, 1, 2]) assert.ok(Math.abs(target[axis] - location.position[axis] - direction[axis]) < 1e-9)
    const back = getLocationOfEditorCamera(camera)
    assert.deepEqual(back.position, location.position)
    assert.ok(Math.abs(back.orientation.theta - location.orientation.theta) < 1e-9 && back.orientation.phi === location.orientation.phi)
    camera.position[0] = 99
    assert.equal(location.position[0], 3, 'The editor camera moves on its own copy')
})

test('a world still on the camera every new world starts with opens framed instead', () => {
    assert.equal(getEditorCameraOfLocation({ position: [0, 100, -100], orientation: { theta: 0, phi: 0.7853982 } }), null)
})
