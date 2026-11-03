This file contains Rust rules, TypeScript rules, and then general guidelines for working in this repo. Also refer to the README.

# Rust rules:

- Avoid using .clone() or .copy() unless absolutely necessary. At the end of your change think about how you can remove .clone() if it would make sense and is easy.
- Factor structs and enums to their fundamental form so that any couplings in their fields are clear. For example, do not write `struct User { is_logged_in: bool, username: Option<String>, }`, instead write `enum User {LoggedIn {username: String}, LoggedOut() }`.
- Ensure items in shared/ are actually shared and not just used in one crate (unless, for some abstraction reason, it is much simpler to group items together in shared/).
- When you change `.wgsl` files, validate diagnostics with `wgsl-analyzer` before finishing.
- Avoid writing .0 if it is part of a struct. Always destructure explicitly.
- Do not remove or change comments unless you were just asked you to implement the thing.
- Test your code before finishing to make sure there are no panics/errors.
- Do not overly break down code into functions. For example, don't declare a function that's used in only one place and can easily just go inline.
- Refer to any relevant official online docs and guidelines before using new external tools or libraries. You must follow existing setup guidelines and avoid setting things up in nonstandard or unoffical ways.
- For any new library, ensure you use the latest possible version.
- Auto format before you end.
- Run `./scripts/stage_test_files` at the end of every turn.


- Locks
- If you use a lock, prefer calling it {lock name}\_lock and then drop({lock name}\_lock) after using it. This makes it clear when it's being accessed. Also be sure to use the lock_conversions file.
- Use tokio locks instead of std locks unless you absolutely need to use std.
- Prefer LazyLock over OnceLock.

- When using a RefCell, always prefer inlining thing_rc.borrow().fn() so that it is auto-dropped. If you can't do this then you *must* make sure the refcell is dropped before any `await`s.

- Occasionally do a treeshake of the Cargo.toml files.

- Try not to nest if statements too much (it's ok if it is simple though). If there are a ton of nested ifs, try using  eg`let Some(x) = y else {return;}`'s instead.

- Don't use `matches!()`, use a match statement instead. Replace it if you see it.


- Never write `#[serde(default)]`.

- Don't add unnecessary macros.


- Avoid `pub(crate)`. Instead, just write `pub`.

- Avoid `pub(super)`. Instead, just write `pub`.

- Never write `pub` on functions/structs/enums that don't actually need to be pub.

- If anything is an `Arc<>`, name it `thing_arc`.

- When you use `Option<>`, _don't_ name it thing_or_none, just name it `thing`.

- Never create a tokio Handle or Runtime or JointHandle when you could just use `#[tokio(main)]`. Simply use the typical standard tokio functions like tokio::spawn() etc. If there is a problem with this simply ask me.

- Avoid convoluted passing of variables like `&mut *variable`.  





# TS rules:
Types: 
- Avoid writing `Map<A,B>` and `Record<A,B>`. Instead, use `{ [k: A]: B }` or `{ [k in A]: B }`.
- Only use `Map` if the keys must be objects.
-  Avoid writing `Array<A>` type. Use `A[]` instead.
-  Avoid writing `any` unless absolutely necessary (very rare)! This includes never writing `as any`, `: any`, etc.
-  Avoid casts (the word `as`) unless absolutely necessary (very rare)! This includes `as any`, `as const`, `as myType`, etc. Type narrowing should usually happen automatically without forcing it with `as`.


    Code design:
    - Break down complex components, hooks, and functions into subfunctions.
    - Never nest component definitions.
    - Never wrap a function and then immediately call it, like `const val = (() => {... return x })()`. If you're doing that, it means you're abstracting incorrectly, and you need to define the variables earlier or make a new intermediate function.
    - Converting an object to a string to use as a key or do a comparison is generally VERY BAD. For example, NEVER use `JSON.stringify(something)` or `object.toString()` as a key to an object. Consider alternatives.
    - Never introduce optional parameters to a function like `x?: Type`. Instead, use the type `Type | undefined`.
    - You don't need to add a new type for a function's parameters. Just inline the type, like `f({x,y}:{x:X, y:Y}) => { ... }`.
    - Prefer using `type`s instead of `interface`s.
    - use arrow syntax `const fn = () => {}`, instead of function syntax.
    - Do not change whitespace formatting. Keep items that are on one line on one line. Follow existing convention for new formatting. Don't introduce any new spacing changes or semicolon changes.

    Styling:
    - Never use `cursor-not-allowed` (or `cursor: not-allowed`). Use `cursor-default` instead, including on disabled controls.

    Comments:
    - Never delete my old comments, and never change my old comments.

    Errors:
    - Never fix unrelated errors to the task you're working on. Just let the user know about them.
    - Never run a server to make sure your changes work, just stop when you're done.



## Rules specific to kube-new:
- This project is a general game maker. Do not hard-code in specific details for one game. The goal is to build a framework, not optimize the engine for one specific game.

- Ensure the TS file is always synced with the jsRustConversions file and the rust structs, including struct names matching TS type names, and their field names matching each other 100%. Also make sure the jsRustConversions file handles the type to struct conversion properly and is not missing any fields, nor has any extras. This is VERY important.

- Ensure all the TS services are updated with one of 3 comments: `/** Only callable by a Server Script. */`, `/** Only callable by a Client Script. */`, or `/** */`, depending on what the ops the service depends on are labeled (comments that are either SERVER, CLIENT, or BOTH).

- In the wgsl files: ensure the renderer.rs struct names and their fields and sizes are 100% the same as in the wgsl structs. 

- If you edit World and want to find objects of a certain type, use the efficient reverse maps instead of iterating through all objects.
- Generally, if you need a reverse map, use the ones in helpers/.

- Env vars.
  - Don't ever read an env var directly, instead put it in `packages/website/.env`, the one public .env file, and add it as a lazy lock entry in `env_vars.rs` and access it through that.
  - Read the README_SECRETS if you need to change a secret. 
  - Ensure every environment variable the Rust code reads appears with the same exact const name in `env_vars.rs`.


- Game Objects:
- Each object type in the game ends with -Info, like BallInfo BrickInfo etc. Those are all the objects.
- ObjectInfo is an enum of all the objects. We use it when passing around a general "object".
- LiveObject is ObjectInfo plus an id and name, it made sense to do this for types in some places.
  All of an object's properties go in its -Info struct, besides children.
- Every object can have children objects. That information does not go in the object's struct, it's stored in world. The only objects with special behavior for their children are Groups. RigidGroup keeps all its children welded as one rigidbody, and LocalGroup just positions everything in the localBasis coordinates initially when the world mounts.
- Some fields point to another object, like followArmatureId in MeshInfo. I added helper structs called -InfoTrackedFields to notate these pointer fields, like MeshInfoTrackedFields. 
  - IMPORTANT: If we change any structs, we ALWAYS to make sure the relevant tracked fields are ALWAYS ACCOUNTED FOR in the InfoTrackedFields structs, and also in ALL THE CODE that uses -InfoTrackedFields. InfoTrackedFields should always be a subset of the properties in Info, with the same exact names and types of those in Info.
- On both the client and the server, we guarantee a parent is always added before its child. Previously we did not assume this, but it is now true.
- See NETWORKING_README if you are dealing with networking.


- When dealing with timestamps that are wall-clock related, follow this convention:
Write the variable or field such that it includes {optional_name} _ {live|dely} _ {client|server}_ms, where the meaning is as follows:
- `live_client`: the client's wall-clock timeline.
- `live_server`: the server's wall-clock timeline, estimated by the client from live_client.
- `dely_client`: the client wall-clock timeline shifted backward by the world delay.
- `dely_server`: the server's wall-clock timeline, estimated by the client from dely_client.
- `sim`: what time it is in the simulation. only ever a whole number of dt from the last tick. Anything whose pose gets divided by dt must be sampled at _sim_ms (animators, skeletons, interpolated network poses)

- Always increment the relevant version in consts.rs before finishing a change. Do not do this unnecessarily or over-cautiously, only when actually needed. It is relevant to check the last time we deployed when bumping these values.





# Conventions


- Variable naming.
  - If there are 2 variants of something, then name them {thing}-{variant} or {variant}-{thing}.  For example, if we have a game server and an editor server, it does not make sense to name one`game-server` and the other `editor`.
  - Ensure variable naming of the same variable is consistent across all usages.
  - Name variables, functions, etc what they actually are. Don't be clever and start using words that are metaphors to what we're doing. Think of the simplest language that you would explain it to a five year old and use similar wording for the name.
  - Booleans should always start with an "is" or "did" or "should", ie sound like a T/F value.
  - Don't name functions or variables vaguely, for example "prepare_import" doesn't tell us what we're preparing about the import. Name functions what they actually do, like "convert_import_to_loaded_gltf". Avoid writing overly-long variable names, just don't be vague.
  - Don't use advanced words like sentinel anywhere. What in god's name is a sentinel. use plain english.

  - Always name maps, objects, and lookup-tables `valueOfKey`. For example, if you have an object that maps from A to B, name it bOfA. If you have an object that maps from users to notes, call it notesOfUser, etc. Do NOT just name an object 'notes' or 'notesToUsers', as these are ambiguous.
  - The `valueOfKey` naming convention should ALWAYS and ONLY apply to maps. For example, a function should only be named `getFrictionOfBrick` if it actually returns a map from brick->friction. If it instead just returns a number, simply name it something like `getFriction` or `getFrictionForBrick`.
  - The naming `bOfA` is explicitly reserved for maps. For example, don't name something which is just a number `heightOfBrick`, since it is not a map.


- If you are implementing a new feature and arbitrarily decide on any values, put a comment `// TODO: AI-PICKED-VALUE: {a brief description of what you picked and how it fits into the codebase or other things you added}`


- Completely ignore backcompatibility as a factor in decision making. It is fine to break production. Simply write the cleanest possible code. Surface backcompatibility warnings to me with all caps in your response, not by writing extra code.

- Helper fns are fine when they actually are used in multiple places, but don't add helper functions that are just used in one place; just write the code in the function where it's used.

- Look for any existing files that implement features you are about to implement, and copy all structure and convention. 


- Comments.
  - If you write a big chunk of code or function, add full-sentence comments above each high-level section that help the reader break it down, using plain english. Don't be too curt or jargony when writing these, they should be easy to read.
  - If a file gets above 300 lines, put a few big comment dividers like // -------------------------- {your small comment} --------------- that describe what each section is generally doing, making it easy for the reader to break down the file.
- Whenever you add or refactor code, write an overview comment in a place that's relevant, and add some general comments above each function or block of code that explains where that function fits into the overview.
- Don't go nuts adding comments, but if you have a big chunk of code it's best to add a few small comments explaining what each chunk is doing. 

- Code order.
  - In any given file, put related code near each other, ie on line numbers that are close. Ideally a file flows top to bottom in the exact dependency order that functions are used, with the main function at the bottom.



# Best practices:

Put new files, functions, consts, etc in the right place, don't just dump them somewhere convenient. Put them on the right line number near the place they're actually used.

- If you define a helper function that is only used in one place, define the it as a nested function inside that function.

- If you are producing a lot of redundant code, abstract it.

- If you add a big new feature, think about what advice you would give me to implement it before implementing it.

- Think hard about the dependency graph and always choose simple abstractions you can build on top of.

- Simplify and change abstractions when necessary, don't just pile code on top of existing abstractions if it doesn't make sense. Treat this like the codebase you own, and actually try to do a good job writing clean maintainable code.

- Don't create functions with parameters that have no effective purpose because they're only called in one or two places. Just remove the params. Abstract in an intelligent way.

- Change the fewest number of line numbers possible. If adding something, add the fewest number of line numbers possible (within reason, don't code golf)

- Don't add backups for things like parameters or env vars. Just assume ONE env var exists, never add fallbacks.

- Avoid using complex attributive adjectives. Prefer simple language and predicative adjectives instead. For example, instead of saying "the per-file comments are", which is very hard to read, say "the comments in each file are", which is much simpler and easier to read.

- If there is an ineffective, redundant, or duplicate abstraction in the codebase, propose a fix and let me know.

- If I ask you a question without directly telling you we should implement something, just respond to the question and don't implement anything.

- If I ask you for a new feature or change, don't just add the smallest hack you can in order to get it to work. Think about the abstractions and whether you're doing the right thing in context. If things aren't adding up, or if you're adding one thing but sacrificing another, it's ok to come back to me and tell me we should really be structuring the code another way instead. 

- It is more important to maintain a good codebase than it is to succeed at exactly what I ask.

- Reference the README.
- The user and other agents may edit or stage code as you edit it. 
- Do not touch Git unless the user asks you to.

- After you add a new functionality or feature, the next step should always be adding tests for it. 
  - Tests should be extensive. Regression tests are very important.
  - Do not add mocks unless I confirm.
  - Make sure tests do not have effects like upload files somewhere. Ask me what to do if you find we need to do something like this to test.

- Always add tests in the tests/ folder. Never add tests in the files themselves with #[cfg(test)].

- Don't couple concerns. For example, if a function is called 'getAuthTokens', don't add update logic to that function, even if it feels convenient, because it is named 'get' and shouldn't also modify. Create a new function with separate functionality, or change the existing function's name if it's doesnt start sounding too overcomplicated.

- Fallbacks are bad. Don't include them. Write code that is minimal.

- When designing a website or new feature, don't add unnecessary filler text, placeholder logic, fallback logic, or anything unnecessary. Just add the bare minimum that looks good.

- Don't add trivial or flaky state-specific tests. Only add tests that actually exist for a reason. 
- For example, if you add a button, don't add a test to ensure a button was added. It is already trivial that a button was added based on the source code. 
- Don't add tests with hard-coded values that will obviously change as time passes.

- Avoid using #[serde(rename = "gameworld_json")] unless you have a good reason. Use r#name instead.

- When developing locally, be sure not to close other editors that are running; only close ones that you started yourself.

- In new Rust code, prefer importing project functions with `use` over long module paths. Do not restyle existing calls just for this preference. Keep conventional standard-library and third-party prefixes where they make sense, such as `tokio::spawn`, `serde_json::to_string`, and `mpsc::channel`.

- Prefer deleting unnecessary state, configuration, feature gates, and alternate code paths over adding scaffolding.


## Guidelines given to you by AI:
Write this like a careful senior engineer with strong boundaries.
Keep concerns separated.
Keep shared state in the owning layer. The code that is in charge of a piece of state should be the code that changes it.
Keep lower-level helpers narrow and mostly pure.
Prefer direct data flow over clever control flow.
Avoid bool-return APIs, nested Option APIs, and vague names.
If the code feels coupled or indirect, refactor it before finishing.
Optimize for obvious ownership, obvious naming, and the fewest necessary moving parts.

Optimize for deleting code, structs, types, and codepaths. When you write code, reduce complexity and add as little code as possible. add *minimal* code, introduce the fewest types/structs/codepaths, keep abstractions simple. write readable simple code that avoids complexity.
