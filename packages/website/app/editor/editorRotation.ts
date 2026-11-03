// Match the engine's rotation order (quat_to_euler_degrees in game_types): y first, then x, then z; storage uses [x, y, z, w].
export const quaternionToDegrees = ([x, y, z, w]: number[]) => {
    const length = Math.hypot(x, y, z, w)
    x /= length; y /= length; z /= length; w /= length
    const sinX = Math.max(-1, Math.min(1, 2 * (x * w - y * z)))
    // At gimbal lock, choose zero Z and put the remaining rotation in Y.
    const isGimbalLocked = Math.abs(sinX) >= 1 - Number.EPSILON
    const angles = [Math.asin(sinX), isGimbalLocked ? Math.atan2(2 * (y * w - x * z), 1 - 2 * (y * y + z * z)) : Math.atan2(2 * (x * z + y * w), 1 - 2 * (x * x + y * y)), isGimbalLocked ? 0 : Math.atan2(2 * (x * y + z * w), 1 - 2 * (x * x + z * z))]
    return angles.map(angle => (angle * 180 / Math.PI + 360) % 360)
}
export const degreesToQuaternion = ([x, y, z]: number[]) => {
    const [sx, sy, sz] = [x, y, z].map(angle => Math.sin((angle % 360) * Math.PI / 360))
    const [cx, cy, cz] = [x, y, z].map(angle => Math.cos((angle % 360) * Math.PI / 360))
    return [cy * sx * cz + sy * cx * sz, sy * cx * cz - cy * sx * sz, cy * cx * sz - sy * sx * cz, cy * cx * cz + sy * sx * sz]
}
