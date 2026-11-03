import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRootPath = fileURLToPath(new URL("..", import.meta.url));

export const repoPath = (...segments: string[]) => path.join(repoRootPath, ...segments);
