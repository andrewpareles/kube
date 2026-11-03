import type { MouseButtonName } from "../clientInputService"

export type GuiSerializedTextNode = {
  text: {
    nodeId: string
    text: string
  }
}

export type GuiElementKind = "div" | "button"

export type GuiSerializedElementNode = {
  element: {
    elementKind: GuiElementKind
    nodeId: string
    className: string
    onClickHandlerId: string | null
    onRightClickHandlerId: string | null
    onMouseDownHandlerId: string | null
    onMouseUpHandlerId: string | null
    onHoverHandlerId: string | null
    children: GuiSerializedNode[]
  }
}

export type GuiSerializedNode = GuiSerializedElementNode | GuiSerializedTextNode

export type GuiRenderedRoot = {
  children: GuiSerializedNode[]
}

export type GuiRootRenderable = GuiRenderable | (() => GuiRenderable)
export type GuiHandler = () => unknown
export type GuiMouseButtonHandler = (button: MouseButtonName) => unknown

/** This is the equivalent to a React Node.
 * 
 * The typical usage is `<MyComponent />`.
 * 
 * You can also use strings, numbers, booleans, null, and undefined.
 */
export type GuiRenderable =
  | GuiJsxElement
  | GuiRenderable[]
  | string
  | number
  | boolean
  | null
  | undefined

export type GuiComponent<Props extends object = {}> = (props: Props & { children?: GuiRenderable | GuiRenderable[] | undefined }) => GuiRenderable

export type GuiJsxProps = {
  children?: GuiRenderable | GuiRenderable[] | undefined
  className?: string | undefined
  onClick?: GuiHandler | undefined
  onRightClick?: GuiHandler | undefined
  onMouseDown?: GuiMouseButtonHandler | undefined
  onMouseUp?: GuiMouseButtonHandler | undefined
  onHover?: GuiHandler | undefined
  style?: never
  styles?: never
  [key: string]: unknown
}

export type GuiJsxElement = {
  tag: string | GuiComponent<object>
  props: GuiJsxProps
}
