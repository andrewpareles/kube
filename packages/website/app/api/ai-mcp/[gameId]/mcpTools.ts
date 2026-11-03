import { z } from 'zod'

// An AI outside kube.new gets the editor's tools from Rust's registry, plus the tools below. The AI chat's own tools,
// for its sandbox and its subagents, live in the editor's Node service, so an outside AI never gets them.

// The editor's own instructions come after this, through read_system_prompt.
export const AI_MCP_NOTE = "You are connected to a kube.new game editor from outside kube.new. Anyone in the web editor can see your changes as you make them, but not your messages. Nothing ends your turn for you, so call `stop_ai_headless_playtest_all` when you are done with a headless playtest. Only one headless playtest runs at a time, shared with the editor's built-in AI: starting one stops the one before it, and the built-in AI stops it whenever it finishes a reply."

// -------------------------- Tools only MCP has --------------------------

type McpToolContext = { inviteToken: string, readSystemPrompt: () => Promise<string>, readEditorUrl: () => Promise<string> }

const toolInputSchema = z.looseObject({ type: z.literal('object') })
const makeMcpTool = <ArgsSchema extends z.ZodObject>({ name, description, argsSchema, run }: { name: string, description: string, argsSchema: ArgsSchema, run: (args: z.output<ArgsSchema>, context: McpToolContext) => Promise<string> }) => ({
    tool: { name, description, inputSchema: toolInputSchema.parse(z.toJSONSchema(argsSchema)) },
    run: (args: unknown, context: McpToolContext) => run(argsSchema.parse(args), context),
})

// A file on the AI's computer cannot reach this server through a tool call, so the file tools answer with a curl command,
// and the AI's own shell sends the file straight to the editor with the game's invite. The editor records what an MCP client
// uploads under this session, the way it records what a browser tab uploads.
const MCP_EDITOR_SESSION = 'mcp'
const quoteForShell = (text: string) => `'${text.replaceAll("'", `'\\''`)}'`
const makeCurlCommand = (inviteToken: string, url: string, options: string[]) =>
    ['curl', '-sS', ...options, '-H', quoteForShell(`Authorization: Invite ${inviteToken}`), '-H', quoteForShell(`x-editor-session: ${MCP_EDITOR_SESSION}`), quoteForShell(url)].join(' ')
const filePathSchema = z.string().describe('Absolute path of the file on your computer.')

// The Import tab takes the same files.
const assetKindSchema = z.enum(['Mesh', 'Texture', 'Skybox', 'Sound']).describe('Mesh for a .glb model, Texture for a .png, .jpg, .jpeg, or .webp image, Skybox for an image of those types that is a panorama twice as wide as it is tall, or Sound for an .ogg, .mp3, .wav, or .flac sound, which is published as an Sfx asset when it is under 10 seconds.')
const shouldPublishSchema = z.boolean().describe('true to also publish it on the Assets page, where anyone can find and use it, or false to only add it to this world. Publishing is encouraged when it is reusable: it lets others use it in their games, and it holds onto a model\'s .glb so you can always get its source later.')
const assetIdSchema = z.number().int().positive().nullable().describe('An asset of the game creator\'s to publish this as the next version of, which needs should_publish true, or null for a new asset.')
// The editor reads a missing asset_id as none.
const makeUploadQuery = ({ should_publish, asset_id }: { should_publish: boolean, asset_id: number | null }) =>
    ({ should_publish: String(should_publish), ...(asset_id === null ? {} : { asset_id: String(asset_id) }) })

// TOOLS THAT WE ONLY EXPOSE TO MCP CLIENTS (and not the inner AI)
export const MCP_ONLY_TOOLS = [
    // read system prompt tool
    makeMcpTool({
        name: 'read_system_prompt',
        description: 'Read the system prompt for working in the editor. Call this before anything else.',
        argsSchema: z.object({}),
        run: async (_args, { readSystemPrompt }) => `${AI_MCP_NOTE}\n\n${await readSystemPrompt()}`,
    }),
    makeMcpTool({
        name: 'asset_import_from_file',
        description: "Add a file from your computer to this world, and also publish it on the Assets page when should_publish is true. A model, texture, or sound is added to the root of the world, and a skybox becomes the world's sky. A texture becomes a Material with only a color image, shown on a new brick. This answers with a curl command; run it in your shell to send the file. The command prints the object_id it added or changed, and the asset_id when it was published.",
        argsSchema: z.object({ file_path: filePathSchema, kind: assetKindSchema, name: z.string(), should_publish: shouldPublishSchema, asset_id: assetIdSchema }),
        run: async ({ file_path, kind, name, should_publish, asset_id }, { inviteToken, readEditorUrl }) => {
            const query = new URLSearchParams({ kind, name, ...makeUploadQuery({ should_publish, asset_id }) })
            return makeCurlCommand(inviteToken, `${await readEditorUrl()}/asset-import?${query}`, ['--data-binary', quoteForShell(`@${file_path}`)])
        },
    }),
    makeMcpTool({
        name: 'asset_import_material_from_files',
        description: 'Add a material made from image files on your computer to the root of this world, and also publish it on the Assets page when should_publish is true. albedo_file_path is its color texture and is required; the normal, roughness, and metallic textures are optional. Use it only when you have a normal, roughness, or metallic image; for a color image alone, import it as a Texture with asset_import_from_file. This answers with a curl command; run it in your shell to send the files. The command prints the new object_id, and the asset_id when it was published.',
        argsSchema: z.object({ name: z.string(), albedo_file_path: filePathSchema, normal_file_path: filePathSchema.nullish(), roughness_file_path: filePathSchema.nullish(), metallic_file_path: filePathSchema.nullish(), should_publish: shouldPublishSchema, asset_id: assetIdSchema }),
        run: async ({ name, albedo_file_path, normal_file_path, roughness_file_path, metallic_file_path, should_publish, asset_id }, { inviteToken, readEditorUrl }) => {
            const filePathOfSlot = { albedo: albedo_file_path, normal: normal_file_path, roughness: roughness_file_path, metallic: metallic_file_path }
            // curl ends a form field's file name at a ; or , unless the name is in double quotes.
            const formOptions = Object.entries(filePathOfSlot).flatMap(([slot, filePath]) => filePath ? ['-F', quoteForShell(`${slot}=@"${filePath.replaceAll(/["\\]/g, '\\$&')}"`)] : [])
            const query = new URLSearchParams({ name, ...makeUploadQuery({ should_publish, asset_id }) })
            return makeCurlCommand(inviteToken, `${await readEditorUrl()}/asset-import-material?${query}`, formOptions)
        },
    }),
    makeMcpTool({
        name: 'asset_download_source_to_file',
        description: "Save the original file of one of the game creator's model assets to your computer, so you can edit it and import it again as that asset's next version. It is the .glb the model was imported from, or a .blend for a model made by kube.new's older Blender tools. This answers with a curl command; run it in your shell to download the file to file_path. The command prints the file's type: model/gltf-binary or application/x-blender.",
        argsSchema: z.object({ file_path: filePathSchema, asset_id: z.number().int().positive() }),
        run: async ({ file_path, asset_id }, { inviteToken, readEditorUrl }) => {
            const query = new URLSearchParams({ asset_id: String(asset_id) })
            // With --fail, a refused download leaves the file alone.
            return makeCurlCommand(inviteToken, `${await readEditorUrl()}/asset-download-source?${query}`, ['--fail', '-o', quoteForShell(file_path), '-w', quoteForShell('%{content_type}\\n')])
        },
    }),
    makeMcpTool({
        name: 'download_world_to_file',
        description: 'Save the current world on the editor server to a .kube file on your computer. This answers with a curl command; run it in your shell to download the world to file_path.',
        argsSchema: z.object({ file_path: filePathSchema }),
        // With --fail, a refused download leaves the file alone.
        run: async ({ file_path }, { inviteToken, readEditorUrl }) => makeCurlCommand(inviteToken, `${await readEditorUrl()}/download-world`, ['--fail', '-o', quoteForShell(file_path)]),
    }),
    makeMcpTool({
        name: 'replace_world_with_file',
        description: "Replace this whole world with a .kube file from your computer. This cannot be undone. This answers with a curl command; run it in your shell to send the file.",
        argsSchema: z.object({ file_path: filePathSchema }),
        run: async ({ file_path }, { inviteToken, readEditorUrl }) => makeCurlCommand(inviteToken, `${await readEditorUrl()}/upload-world`, ['--data-binary', quoteForShell(`@${file_path}`)]),
    }),
]
