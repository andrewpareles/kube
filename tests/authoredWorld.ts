// game-35-latest.kube predates isPushable, which replaced isAnchored. The editor server hands the browser worlds in the current
// shape, so a test that reads the file itself changes it to that shape first.
export const withIsPushable = <StoredWorld extends { physicsToggles3DOfId: Record<string, { isAnchored?: boolean, isPushable?: boolean }> }>(storedWorld: StoredWorld) => {
    for (const toggles of Object.values(storedWorld.physicsToggles3DOfId)) {
        toggles.isPushable = !toggles.isAnchored
        delete toggles.isAnchored
    }
    return storedWorld
}
