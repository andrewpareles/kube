import type { GuiComponent, GuiJsxProps, GuiRenderable } from "./gui/guiTypes";

// `jsx-runtime` is a special automatic JSX entry name. TypeScript resolves
// `<jsxImportSource>/jsx-runtime`, so keep this filename and export path literal.

// We use this string to tell when JSX came from <>...</> in this file.
export const Fragment = "__kubeTsApiJsxFragment__";

// This keeps JSX children in the same small runtime shape whether there are zero, one, or many.
const normalizeChildren = (children: GuiRenderable[]): GuiRenderable | GuiRenderable[] | undefined => {
  if (children.length === 0) {
    return undefined;
  }
  if (children.length === 1) {
    return children[0];
  }
  return children;
};

const createGuiJsx = (tag: string | GuiComponent<object>, props: GuiJsxProps | null) => {
  const nextProps: GuiJsxProps = {
    ...(props ?? {}),
  };
  if ("style" in nextProps || "styles" in nextProps) {
    throw new Error(
      "Use className for GUI styles. style and styles are not supported.",
    );
  }

  const explicitChildren = nextProps.children;
  const mergedChildren = Array.isArray(explicitChildren)
    ? normalizeChildren(explicitChildren)
    : explicitChildren;

  if (tag === Fragment) {
    return mergedChildren ?? null;
  }

  return {
    tag,
    props: {
      ...nextProps,
      children: mergedChildren,
    },
  };
};

export const jsx = (tag: string | GuiComponent<object>, props: GuiJsxProps | null, _key?: unknown) => {
  return createGuiJsx(tag, props);
};

export const jsxs = (tag: string | GuiComponent<object>, props: GuiJsxProps | null, _key?: unknown) => {
  return createGuiJsx(tag, props);
};

export namespace JSX {
  export type Element = GuiRenderable;

  export interface ElementChildrenAttribute {
    children: {};
  }

  export interface IntrinsicElements {
    div: GuiJsxProps;
    button: GuiJsxProps;
  }
}
