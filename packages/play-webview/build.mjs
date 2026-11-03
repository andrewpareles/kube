import { build } from "esbuild";
import path from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = fileURLToPath(new URL(".", import.meta.url));

// One self-contained file that the desktop client builds into itself and serves to its webview.
await build({
  bundle: true,
  entryPoints: [path.join(packageRoot, "src/index.tsx")],
  format: "esm",
  loader: {
    ".css": "text",
  },
  minify: true,
  outfile: path.join(packageRoot, "dist/play-webview.bundle.js"),
  platform: "browser",
  target: "es2022",
  // JSX compiles with tsconfig.json's settings. Without the file, esbuild would quietly compile it for a global React
  // the page doesn't have, so the page would draw nothing.
  tsconfig: path.join(packageRoot, "tsconfig.json"),
});
