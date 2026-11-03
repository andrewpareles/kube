// !!!!!!!!!!!!!!!! refer to https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide/#parallel-tool-calling

// Keep the hand-authored prompt in Markdown while appending the generated script types.
pub const SYSTEM_PROMPT: &str = concat!(
    include_str!("system_prompt.md"),
    "\n",
    include_str!("system_prompt_uploads_addon.md"),
    "\n",
    // types
    "Any tools that reference the TypeScript types are referencing this bundled declaration file:",
    "kube-ts-api.api.d.ts:\n",
    "```ts",
    include_str!("../../../../packages/kube-ts-api/dist/kube-ts-api.api.d.ts"),
    "```"
);
