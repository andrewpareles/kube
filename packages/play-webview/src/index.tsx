import { createRoot } from "react-dom/client";
import { postToHost } from "./hostBridge";
import { startHandlingPageInput } from "./pageInput";
import { PlayWebViewPage } from "./PlayWebViewPage";
import { startTrackingRightMouseDown } from "./ScriptGuiRoots";
import { startTailwind } from "./tailwindRuntime";
import { startReportingUiRectsToHost } from "./uiRects";

const container = document.getElementById("root");

if (container === null) {
  throw new Error("Missing #root container.");
}

startHandlingPageInput();
startTrackingRightMouseDown();
// The same fonts the website uses. The desktop client serves these files itself, and the editor server writes them next
// to the page. They are added once and loaded before anything is drawn. Declared in the Tailwind stylesheet instead, they
// were fetched again every time a new class rebuilt it, and the text flickered to a fallback font until they came back.
const fontFaces = [
  new FontFace("Inter", "url(fonts/InterVariable.ttf)", { weight: "100 900" }),
  new FontFace("Inter", "url(fonts/InterVariable-Italic.woff2)", { weight: "100 900", style: "italic" }),
  new FontFace("Geist Mono", "url(fonts/GeistMono-Regular.ttf)", { weight: "400" }),
];
await Promise.all([startTailwind(), ...fontFaces.map((fontFace) => fontFace.load())]);
for (const fontFace of fontFaces) {
  document.fonts.add(fontFace);
}
createRoot(container).render(<PlayWebViewPage />);
startReportingUiRectsToHost();
postToHost("ready");
