
This file goes over common conventions we use in the kube.new engine.

See AGENTS.md for implementation guidelines, and also reference the other README's when dealing with specific parts of the codebase.

## Rust
Notation:
  - For the LiveObject structs, if it ends with:
  - Info => it's a variant of an object that tracks almost everything about the object and is sent across the network.
    - Each Info struct gets its own rigidbody if it's physical, and has ONE position (if any) sent across the network.
  - Data => static data that lives on the cloud and is fetched from a URL.

Data
- Assume there's a URL that the raw binary data lives at, and we can just call that URL and parse the data.

ObjectIdOwned and ObjectIdPointer
- ObjectIdOwned is an "owned" pointer to an object, meaning it can only be referenced by 1 object.
  - You can't delete an owned item.
  - We must always upkeep the owned invariant. 
    - This means if an object is deleted that owns other items, it should no longer be marked as owning those items (done in removeObject).
    - If an object is added and it says it owns items, we must update our ownership tracking (done in addObject).
    - If an object's field changes and it's of type ObjectIdOwned, we must update our ownership tracking (by calling setOwnerOfItem).

- ObjectIdPointer is just a normal pointer to an object.
  - We make sure it has no references before it's allowed to be removed, and don't do anything else special for it.

- Another way of looking at ObjectIdOwned and ObjectIdPointer:
  - ObjectIdOwned means the thing can only be owned by one object.
  - ObjectIdPointer means the thing can be owned by many objects.

  - you can't delete an item that's owned/pointed to aka shows up in an ObjectIdOwned | ObjectIdPointer, so never need to worry about reference issues.
  - when you delete an object, we have internal ownership/pointer information that we update.



Other notes:
- RigidGroup is to be treated as a rigid body with multiple colliders in it. If nested, we treat the highest one as the true "rigid group" as a fallback, but should ideally never be nested.



## JS Runtime Types

- There are TS files which define the contract exposed to scripts in our jsRuntime.rs files. Make sure the types in both files are ALWAYS consistent with each other.
- As a rule of thumb, keep the types in types.ts identical to the the LiveObject structs. For example, BallInfo should have the same type as the ball type. Any exceptions must be labeled with the comment "// DIFFERENT:" in the .ts file. For example, if we spread certain fields in the TS types for ease of access, that is fine but must be indicated with a comment.

## Coordinate System

```

        +Y
         ^    +Z
         |   /     [XZ plane is the floor]
         | /
  -X <---+---> +X (right)
         |
         |
         v
        -Y

NOTE: +Y = up, -Y = down.
Handedness: Right-handed.
```

Rust stores theta/phi and quaternion math in radians internally.
JS-facing theta/phi, rotations, and angular velocities use degrees.
Theta is cylindrical angle in XZ plane, going clockwise, starting along +Z.
Phi is stored in radians internally and exposed in degrees on the JS side. phi = 0 means looking horizontally. Rust-internal phi is positive down, and JS-facing Orientation3D phi is positive up.

## WGPU notes

- bindgrouplayout = the shape of @group(n) @binding(m)
- bindgroup = actual data for the layout
A binding is a pointer to a buffer/texture/sampler.
You create buffers, then bind them so that the @group @binding now points to them.
For textures, you bind views to textures (not raw textures).

Sun rendering, lighting, and shadows:
- WorldLighting.sunOrientation is the shared source of truth for the sun direction.
- Direct sun lighting is directional. Every shaded point uses the same sun direction, with no sun position and no distance falloff. This treats the sun as infinitely far away.
- Sun shadows are also directional. The shadow pass renders opaque objects into cascaded shadow maps using orthographic cameras looking along the same sun direction. This is not a grid of rays from the sun; it is depth textures rendered from the light's point of view, then sampled by the main scene shader.
- The cascades are centered on the current render center and split into close, step-up, medium, far, and very-far maps. Close objects get a smaller world area at higher resolution, while far shadows still cover a large area around the render center.
- The visible sun in the sky is only a visual marker drawn by sky_sun.wgsl. Its size does not affect lighting or shadow softness. Its glow comes from bloom: the glow pass draws a soft disk around it wherever the sky shows, so the glow spills over whatever hides part of the sun.
- The current visible sun core radius is intentionally game-readable, not physically correct. With the current camera FOV it is much larger than the real sun's angular size.
- Shadow softness blurs every shadow edge by the same world distance, `WorldLighting.sunShadowSoftness`, however far it is from what casts it. The blur reads the cascade whose texels suit its width.
- So lighting and shadows agree on direction, but the visible sun disk size is decorative.
- Smooth and metal surfaces reflect the sky: the gradient, with the skybox image over it. Rougher surfaces read a blurrier skybox mip and lean toward the ambient light, and surfaces that are not metal fade out of it between roughness 0.25 and 0.6, so plain bricks stay matte. `WorldLighting.skyReflectionStrength` scales every sky reflection. Nothing blocks the sky, so glossy floors under a roof still reflect it.
- Vibrance runs right after the tone curve, in the same shared display conversion: it pushes each lit color further from gray by `WorldLighting.vibrance` times how dull the color is, so vivid colors barely change. Labels and skybox images keep their exact colors.
- Glow (`glow` on every object's visual settings) makes a surface show its own color unlit, and bloom spreads it: a glow pass draws only glowing instances, label text and the sun at half the picture's size, straight into the first bloom level. That level is too small to share the base layer's depth buffer, so the glow shaders read the base layer's depth and test it themselves, and walls still hide what is behind them. bloom.wgsl then halves that level five more times and grows it back, adding each level onto the one above it, and the base layer's composite adds the result as it puts the layer on the picture, before the always-on-top layer draws. A frame with nothing glowing skips all of it.



## Texture map notes
Andrew explanation of TBN, tangents, normals, texture maps:
N = Normal = the orthogonal direction to the triangle face (or the vertex). 
    Normals live on vertices, not triangles, but if you interpolate 3 vertices' normals you get a normal for that triangle. The renderer interpolates per-pixel so is more fine detail than just flat triangles.
    For a normal map ("tangent-space normal map", aka the only type of normal texture most other platforms support), the normal map represents fine detail bumps on top of a triangle face. 
T and B are perfectly orthogonal to N and sit on the triangle face too. They describe "tangent space", aka the orientation of the textures/UVs (same thing) on the triangle face.
    They're just used internally by the shader for efficient computation. 
    They are reverse-engineered from the UVs of the mesh, and then made to lie flat on the triangle by projecting down using N. A tangent map provided by a user is not a thing.



## Other

If any wording in this README is unclear or ambiguous or references something that does not exist, stop immediately and let me know.


## Serialize
We always want to serialize option fields
Apply this find and replace every so often, make sure match case is on
(i made the E upper case so we dont accidentally find and replace the string below)
```re
#\[derive\((.*?)Serialize(.*?)\)\]

#[serde_with::skip_serializing_none]
#[derive($1SErialize$2)]
```

- Prefer serde's normal enum representation. Only use a custom enum JSON shape when an external protocol requires it.
- Do not do this on unit enums nor enums that are "mostly" unit enums. 


## Database

- The source of truth for the database is DRIZZLE, aka schema.ts and relations.ts.
  - Never use supabase CLI to do a migration, use drizzle instead.
- We push with drizzle, inside of packages/website.
  - We never pull with drizzle or it messes up the type files that we painstakingly organized.

To pull new types and do a drizzle push/migration, simply use the npm scripts in packages/website, eg `npm run supabase-typegen` or `npm run drizzle-push`. Never use another command without confirming with the user.



## QUIC Certs
We self-sign our own certs for the server/client QUIC connection. We don't want to deal with Let's Encrypt right now so just do this instead.
The certs expire in year 4096... If we want to rotate certs we just need to change the certs and then push a client update so client has the new cert. That just means we control our own cert expiration.
To rotate them, run `cargo run -p server --bin generate_quic_certs` (or go the file and click run). It writes the new cert to `certs/quic_server_cert.der` and prints the new key, which goes in `QUIC_SERVER_KEY_DER_BASE64` in Infisical's Production `/vercel` (see [Secrets](#secrets)).
Every server reads that key, local ones included. Local playtests get it from `.env.local` through the website, and a server you run by hand needs it in your shell (`infisical run --env=dev -- cargo run -p server`). `join_tests` and `quic_cert_bundle_tests` start a real server too, so run the server's tests with `infisical run --env=dev -- cargo test -p server`. The other QUIC tests make their own throwaway certs.



## Secrets (ai-written)
Secrets are never in git. The one committed env file, `packages/website/.env`, only holds public config. Every secret is stored in Infisical and synced to Vercel and GitHub. [README_SECRETS.md](README_SECRETS.md) lists every secret and where it lives:
- Website: Vercel's environment variables, for Production and Preview, synced from Infisical.
- Game server and editor server: they store none. The website passes their secrets from Vercel as hidden variables when it starts them on Edgegap.
- Your computer: `packages/website/.env.local`, which git ignores. `infisical export` writes it from Infisical's Development environment, and `.worktreeinclude` copies it into every new Claude Code worktree.
- CI: the GitHub secrets below, synced from Infisical. Builds need no other secrets.



## Github Secrets
To set, add it to Infisical's Production `/github`, which syncs it here.
To read, run `gh secret list`

See [README_SECRETS.md](README_SECRETS.md) for a list of the secrets we track.

## Auto updates
We use cargo-packager-updater to do auto updates. Be sure to update the env var `UPDATER_PUBLIC_KEY` if we ever rotate the keys!

Generate using `openssl rand -hex 32` and `cargo packager signer generate`, then put both in Infisical's Production `/github` as `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD` and `CARGO_PACKAGER_SIGN_PRIVATE_KEY`, which syncs them to GitHub.


## Player controller
Player movement is controlled locally by each player, by the `CharacterController` client script every world starts with (`assets/scripts/character_controller.ts`; a world migration adds it to older worlds). It runs in `game.clientCharacterService` step listeners. Its movement numbers are at the top of the script, and a server script can change one player's with the script's `setMovement` client function. Shift toggles shift lock, which locks the mouse, turns the cursor into a crosshair and moves the camera over the character's right shoulder with `setCameraOffset`.

There are 4 main player controllers, run before each physics step:
- A righting force. It's a critically damped "angle spring" (2nd order) that changes the angular velocity.
- A player direction orientation controller. It's a critically damped "angle spring" (2nd order) that keeps its own spin and sets the angular velocity to it, so the floor's friction can't slow the turn. In shift lock it faces where the camera looks instead, spinning at the angle left to turn divided by 0.04 s, so it keeps up with the camera.
- A velocity-based linear momentum controller. It changes the velocity by a max amount per second, towards the desired target.
- An upward velocity change controlled by JumpState. JumpState tracks things like air jump, etc.

After each physics step, the script also does postprocessing for the character:
- Sticky floors - If they are inside the ground or slightly above it, we project them right onto the surface. This makes sure they they stick to ramps and don't clip slightly into platforms when landing and jolt.
- Stairs - Walking down stairs already works fine. We just need to fix stub toe. If they are in a block and can be teleported up by a small step-size distance, we teleport them up.

In physics, we also:
- Remove friction so player can't stick to walls.




## Versions
### Pushing an update
1. bump the version in Cargo.toml
2. commit
3. ./scripts/push_next_release.sh


### Edgegap versions 
- clicking 'play' on the website always starts servers on edgegap that have the `prod` version.
- Github actions only ever touch the `prod` version, never any other version. They patch its `docker_image` and `docker_tag` to point at the image they just pushed.
  - `docker_image` is patched too (not just the tag) so that renaming `EDGEGAP_IMAGE_NAME` in the workflow is enough to repoint `prod`. The tradeoff is that a wrong `EDGEGAP_IMAGE_NAME` now silently repoints `prod` at an image that does not exist, which CI cannot do when it only patches the tag.
- the Edgegap application is `kube-new-server`, and Github actions push the server to its existing `candy-cane-games-yonqr3v74kch` registry project.
- Run `./scripts/configure_edgegap_version.sh <version-name> <docker-tag>` the first time you create an Edgegap version, for example when bootstrapping `prod`.
  - Edgegap settings associated with versions, like ports, CPU, memory, max duration are configured here.
  - If you need to change one of those static settings later, do not change it in CI. Recreate the Edgegap version with the script when it has no running deployments.


### server state in supabase
  - on_ready and polling -> add to the supabase db - whichever is earlier.
  - on_terminate and SIGTERM -> remove from the supabase db - whichver is earlier.

client always checks for updates before launching, so client should typically never outdated. server might be, though.


## Local website and editor

Run the website with local editor and playtest support:

```bash
./scripts/run_website.sh
```

On Windows, use `scripts/windows-versions/run_website.ps1`. See the [dev website setup](packages/website/README.md#dev-website-and-editor) for database setup and required credentials.

To run only the website without rebuilding assets or the browser renderer:

```bash
./scripts/run_only_website.sh
```


## Agent skills

See AGENTS.md for how to use the skills.

- The agent skills use a symlink:
```
kube-new/
├── .agents/
│   └── skills/                          ← REAL FILES live here
│
├── .claude/
│   ├── skills  ──────────────────────►  ../.agents/skills      (THE ONLY SYMLINK)
│   ├── settings.local.json              real file, Claude-only
│   └── worktrees/                       real dirs, Claude-only (66GB)
│
└── AGENTS.md                            plain text pointer, no link
```

## Analytics

The website and the game server send events to PostHog (project "Website - Kube.new"). Name events `{surface}-{thing}-{action}`, where surface is `editor`, `site` or `app`, like `editor-chat-submit` or `site-avatar-save`. Logged-in users are identified by their numeric user id. Play time comes from the game server, which sends `app-game-leave` with `duration_seconds` when a player leaves an official game. It also sends `app-game-join` when a player joins one.


## Misc naming

Naming: `Chat` / `chat` refers to AI conversations. `GameChat` / `game_chat` refers to chat between players in a running game.



## Installing Deps

### Mac

Do this to install all deps. It's fast so we just do everything together:
`./scripts/install_deps.sh`

For hosted editor setup, see the [deployment instructions](crates/editor_server/README.md#deploy).

Editing opens in the browser, using [the Rust editor server and Node AI chat together on Edgegap](crates/editor_server/README.md). The [website API](packages/website/README.md#editor-hosting-api) handles deployment startup. The desktop app runs Play and separate Playtest players; a blank launch opens the homepage.

### Windows (PowerShell)

Install Node.js/npm and Rust through rustup first, including the Visual Studio C++ build tools required by Rust on Windows. From the repository root, run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-versions\install_deps.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-versions\run_website.ps1
```

All Windows-specific scripts live in `scripts/windows-versions/`, including the browser renderer build and client packaging script. The installer installs the npm packages, rebuilds the JavaScript bundles, and installs and builds the WebAssembly renderer. The website's npm dev/build hooks select the renderer script for the current platform automatically.

