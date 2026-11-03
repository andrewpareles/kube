import { compile } from "tailwindcss";
import tailwindIndexCss from "tailwindcss/index.css";
import playWebViewCss from "./playWebView.css";

// Scripts build their class names while they run (a meter's `w-[37%]` changes every frame), so the
// page cannot know its classes ahead of time. Instead it runs Tailwind's own compiler, and whenever
// the page changes it adds CSS for any class that does not have CSS yet.

// TODO: AI-PICKED-VALUE: Once the page has asked for this many classes, the compiler is replaced by a
// fresh one that only knows the classes still on the page. Every new class makes Tailwind write out the
// whole stylesheet again, so a stylesheet that only ever grows would get slower for as long as a meter
// keeps making new widths. The Rust class cache this replaces was capped at the same number.
const MAX_BUILT_CLASS_NAME_COUNT = 4096;

type TailwindCompiler = Awaited<ReturnType<typeof compile>>;

const createTailwindCompiler = () =>
  compile(playWebViewCss, {
    loadStylesheet: async (id: string, base: string) => {
      if (id !== "tailwindcss") {
        throw new Error(`The play webview cannot import "${id}".`);
      }
      return { path: id, base, content: tailwindIndexCss };
    },
  });

const stylesheet = new CSSStyleSheet();
let tailwindCompiler: TailwindCompiler | undefined = undefined;
let builtClassNames = new Set<string>();
let isReplacingTailwindCompiler = false;

// What a group of rules is under, like `@layer utilities` or `@media (hover: hover)`. Any other kind of group goes by its
// whole text, so it is only ever kept or replaced whole.
const getGroupHeader = (group: CSSGroupingRule) => {
  if (group instanceof CSSLayerBlockRule) {
    return `@layer ${group.name}`;
  }
  if (group instanceof CSSConditionRule) {
    return `${group.constructor.name} ${group.conditionText}`;
  }
  return group.cssText;
};

// Changes the `live` rules until they match the `built` ones, without touching any live rule that was built again, so the
// browser never redoes what it set up for it, like loading a font or decoding an image. Tailwind writes out the whole
// stylesheet for every new class, and putting all of that up again made text flicker while its font reloaded.
const updateRules = (live: CSSStyleSheet | CSSGroupingRule, built: CSSStyleSheet | CSSGroupingRule) => {
  const liveTexts = Array.from(live.cssRules, (rule) => rule.cssText);
  const builtTexts = Array.from(built.cssRules, (rule) => rule.cssText);
  const builtTextSet = new Set(builtTexts);
  // Rules go in Tailwind's order, since a later rule wins over an earlier one. Once a rule is handled, the live rules match
  // the built ones up to it, so both share its index.
  for (const [index, builtText] of builtTexts.entries()) {
    // The rule is already up. Any live rules before it were not built this time, like ones for classes that left when
    // the compiler was replaced, so they are removed.
    if (liveTexts.includes(builtText, index)) {
      while (liveTexts[index] !== builtText) {
        live.deleteRule(index);
        liveTexts.splice(index, 1);
      }
      continue;
    }
    // A group whose rules changed, like `@layer utilities` after a new class, is changed in place. A live group that is
    // built again further on is left for later, like the `@media (hover: hover)` group each hover class gets.
    const liveRule = live.cssRules.item(index);
    const builtRule = built.cssRules.item(index);
    if (liveRule instanceof CSSGroupingRule && builtRule instanceof CSSGroupingRule && !builtTextSet.has(liveTexts[index]) && getGroupHeader(liveRule) === getGroupHeader(builtRule)) {
      updateRules(liveRule, builtRule);
      continue;
    }
    live.insertRule(builtText, index);
    liveTexts.splice(index, 0, builtText);
  }
  while (live.cssRules.length > builtTexts.length) {
    live.deleteRule(builtTexts.length);
  }
};

// Page changes are handled before the browser paints, so new classes never show up unstyled.
const buildTailwindForPage = () => {
  if (tailwindCompiler === undefined) {
    return;
  }
  const newClassNames: string[] = [];
  for (const element of document.querySelectorAll("[class]")) {
    for (const className of element.classList) {
      if (builtClassNames.has(className)) {
        continue;
      }
      builtClassNames.add(className);
      newClassNames.push(className);
    }
  }
  if (newClassNames.length === 0) {
    return;
  }
  const builtStylesheet = new CSSStyleSheet();
  builtStylesheet.replaceSync(tailwindCompiler.build(newClassNames));
  updateRules(stylesheet, builtStylesheet);

  if (builtClassNames.size <= MAX_BUILT_CLASS_NAME_COUNT || isReplacingTailwindCompiler) {
    return;
  }
  // The old stylesheet stays up while the new compiler starts, and it already covers the page.
  isReplacingTailwindCompiler = true;
  void createTailwindCompiler().then((nextTailwindCompiler) => {
    tailwindCompiler = nextTailwindCompiler;
    builtClassNames = new Set();
    isReplacingTailwindCompiler = false;
    buildTailwindForPage();
  });
};

export const startTailwind = async () => {
  tailwindCompiler = await createTailwindCompiler();
  document.adoptedStyleSheets.push(stylesheet);
  buildTailwindForPage();
  new MutationObserver(buildTailwindForPage).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class"],
    childList: true,
    subtree: true,
  });
};
