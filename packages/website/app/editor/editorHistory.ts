// Undo and redo belong to the editor server, which owns the world and the shared history. The
// browser only turns a keystroke into a request; the resulting world arrives through the normal
// change stream, the same way an AI edit or another person's edit does.

// Monaco in Chrome types into a div with EditContext, whose isContentEditable is false.
const isTextEntryTarget = (target: EventTarget | null) => target instanceof HTMLElement && (target.isContentEditable || target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.closest('.monaco-editor, [role="textbox"]') !== null)
// A closed dropdown in the inspector is a combobox, and undo still applies there.
const isPopupTarget = (target: EventTarget | null) => target instanceof HTMLElement && target.closest('[role="dialog"], [role="alertdialog"], [role="listbox"], [role="menu"]') !== null

// cmd+z undoes, and cmd+shift+z or cmd+y redoes, with ctrl for Windows and Linux. Text controls
// keep their own undo, which covers the chat box, property text fields, and the script editor.
export const getEditorHistoryDirection = (event: KeyboardEvent): 'undo' | 'redo' | null => {
    if (event.altKey || !(event.metaKey || event.ctrlKey)) return null
    if (isTextEntryTarget(event.target) || isPopupTarget(event.target)) return null
    const key = event.key.toLowerCase()
    if (key === 'y') return 'redo'
    if (key !== 'z') return null
    return event.shiftKey ? 'redo' : 'undo'
}

export const getEditorSelectionShortcut = (event: KeyboardEvent): 'select-root' | 'frame' | null => {
    if (event.isComposing || event.defaultPrevented || isTextEntryTarget(event.target) || isPopupTarget(event.target)) return null
    if (event.target instanceof HTMLElement && event.target.closest('[role="combobox"]') !== null) return null
    const key = event.key.toLowerCase()
    if (key === 'a' && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey) return 'select-root'
    if (key === 'f' && !event.metaKey && !event.ctrlKey && !event.altKey && !event.repeat) return 'frame'
    return null
}
