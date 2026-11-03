// The editor camera moves freely; its viewing direction does not depend on a world object or orbit target.
export type EditorCamera = { position: [number, number, number], yaw: number, pitch: number }

export const getEditorCameraTarget = ({ position, yaw, pitch }: EditorCamera): [number, number, number] => [
    position[0] - Math.sin(yaw) * Math.cos(pitch),
    position[1] - Math.sin(pitch),
    position[2] - Math.cos(yaw) * Math.cos(pitch),
]

// Keyboard movement and scrolling translate the same camera. Diagonal movement keeps the same speed.
export const moveEditorCamera = (camera: EditorCamera, forward: number, right: number, up: number, distance: number) => {
    const length = Math.hypot(forward, right, up)
    if (length === 0) return
    const { position, yaw, pitch } = camera
    const step = distance / length
    // Up follows the camera's pitch and stays perpendicular to its viewing direction.
    position[0] += (-Math.sin(yaw) * Math.cos(pitch) * forward + Math.cos(yaw) * right - Math.sin(yaw) * Math.sin(pitch) * up) * step
    position[1] += (-Math.sin(pitch) * forward + Math.cos(pitch) * up) * step
    position[2] += (-Math.cos(yaw) * Math.cos(pitch) * forward - Math.sin(yaw) * right - Math.cos(yaw) * Math.sin(pitch) * up) * step
}

// A world's saved camera faces +Z at theta 0, where the editor camera faces -Z at yaw 0. A world still
// on the camera new worlds used to start with has no view worth opening at, so the editor frames it instead.
type CameraLocation = { position: [number, number, number], orientation: { theta: number, phi: number } }
export const getEditorCameraOfLocation = ({ position, orientation }: CameraLocation): EditorCamera | null => {
    const isNewWorldCamera = position[0] === 0 && position[1] === 100 && position[2] === -100 && orientation.theta === 0 && Math.abs(orientation.phi - Math.PI / 4) < 1e-6
    return isNewWorldCamera ? null : { position: [position[0], position[1], position[2]], yaw: orientation.theta - Math.PI, pitch: orientation.phi }
}
export const getLocationOfEditorCamera = ({ position, yaw, pitch }: EditorCamera): CameraLocation => ({ position: [position[0], position[1], position[2]], orientation: { theta: yaw + Math.PI, phi: pitch } })
