// Other clients and the AI edit the same world, so the browser cannot tell whether leaving loses work. It always asks.
export const confirmEditorLeave = () => window.confirm('Leave the editor? Your changes may not be saved.')

// A desktop app link leaves this page where it is. The browser fires beforeunload inside assign, so only that call skips the warning.
let isOpeningAppLink = false
export const openAppLink = (url: string) => {
    isOpeningAppLink = true
    try { window.location.assign(url) } finally { isOpeningAppLink = false }
}

// Full page exits use the browser's warning. History traversal within the app needs its own guard.
export const listenForEditorLeave = () => {
    // TypeScript's DOM library does not yet include the Navigation API.
    type EditorNavigateEvent = Event & { navigationType: string, destination: { sameDocument: boolean }, hashChange: boolean }
    const browser: Window & { navigation?: { addEventListener: (type: 'navigate', listener: (event: EditorNavigateEvent) => void) => void, removeEventListener: (type: 'navigate', listener: (event: EditorNavigateEvent) => void) => void } } = window
    const beforeUnload = (event: BeforeUnloadEvent) => {
        if (isOpeningAppLink) return
        event.preventDefault()
        event.returnValue = ''
    }
    const navigate = (event: EditorNavigateEvent) => {
        if (event.navigationType !== 'traverse' || !event.destination.sameDocument || !event.cancelable || event.hashChange) return
        if (!confirmEditorLeave()) event.preventDefault()
    }
    window.addEventListener('beforeunload', beforeUnload)
    browser.navigation?.addEventListener('navigate', navigate)
    return () => {
        window.removeEventListener('beforeunload', beforeUnload)
        browser.navigation?.removeEventListener('navigate', navigate)
    }
}
