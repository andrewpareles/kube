// Pointer tracking distinguishes selection taps from camera gestures, including a drag that returns
// to its starting point. Losing capture or focus cancels the gesture by removing its pointer.
export type EditorPointer = { x: number, y: number, startX: number, startY: number, isDragging: boolean, isClick: boolean }
// TODO: AI-PICKED-VALUE: Allow four CSS pixels of hand movement before a click becomes a camera drag.
const dragThreshold = 4

export const beginEditorPointer = (pointerOfId: { [id: number]: EditorPointer }, pointerId: number, x: number, y: number, button: number) => {
    const pointers = Object.values(pointerOfId)
    for (const pointer of pointers) { pointer.isClick = false; pointer.isDragging = true }
    pointerOfId[pointerId] = { x, y, startX: x, startY: y, isDragging: pointers.length > 0, isClick: button === 0 && pointers.length === 0 }
}

export const moveEditorPointer = (pointer: EditorPointer, x: number, y: number) => {
    const wasDragging = pointer.isDragging
    pointer.isDragging ||= Math.hypot(x - pointer.startX, y - pointer.startY) > dragThreshold
    const dx = pointer.isDragging ? x - (wasDragging ? pointer.x : pointer.startX) : 0
    const dy = pointer.isDragging ? y - (wasDragging ? pointer.y : pointer.startY) : 0
    pointer.x = x; pointer.y = y
    return { dx, dy }
}

export const finishEditorPointer = (pointerOfId: { [id: number]: EditorPointer }, pointerId: number, x: number, y: number) => {
    const pointer = pointerOfId[pointerId]
    if (!pointer) return null
    moveEditorPointer(pointer, x, y)
    delete pointerOfId[pointerId]
    return pointer.isClick && !pointer.isDragging ? { x: pointer.x, y: pointer.y } : null
}
