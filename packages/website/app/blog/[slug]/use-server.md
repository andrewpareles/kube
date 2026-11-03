---
title: 'use-server' Deep Dive
description: How we implemented our own version of 'use server' and 'use client'.
isPublished: true
date: 2026-08-20
priority: 5
author_id: 17
author_name: Andrew Pareles
thumbnail_image_url: /blog/desktop-app/thumbnail.svg
---

Kube.new lets you write scripts in TypeScript. See our [Why TypeScript](/blog/why-ts) blog post for why.

However, TypeScript doesn't come with communication between the server and the client. That's what we're going to discuss implementing in this blog.



## Script Communication

How do Server-side Scripts talk with Client-side Scripts?

If you've ever used React, you know clients are allowed to call functions on the server. You write `'use server'` at the top of the server script's file, and React exposes those exports to the client. So we simply copy this design pattern and build an implementation of it.

We implement both `'use server'`, and its nonstandard cousin `'use client'`. First, let's talk about `'use server'`, because it's slightly simpler. 


### Example - 'use server'
Suppose you're a developer building a game in Kube.new, and you're writing your first scripts. You want your client to ask the server for a player's score. We allow you to write this on the client:

```tsx
// Client script:
import { getScore } from '../the-script-on-the-server'

let GameUI = () => {
    let score = getScore() // <- From server!
    return (<div> Your score is: {score} </div>);
}
render(<GameUI />)
```

Simply write a server script that exposes `getScore` to the client:
```ts
// Server script:
'use server'

export const getScore = (userid: number): number => { return scoreOfUser[userid] ?? 0 }
```

This is standard `'use server'` behavior. The question is, how do we actually implement this internally? 

### Implementing 'use server'

It's easiest to jump directly into the implementation.

Suppose this is our Server script:
```ts
'use server'

export const fn_1 = () => { ... }
export const fn_2 = () => { ... }
export const fn_3 = () => { ... }
export const fn_4 = () => { ... }
```

There are 2 sides on which this script will be read: the Server and the Client.

When the script is read on the server, we keep the file almost exactly the same, except we also append a few lines of `__registerJsServerFn__`.
The `__registerJsServerFn__` function is implemented behind the scenes in Rust (we register it as a Deno op).

This lets us store each TS function by name on the server as a TS Closure that we can run from Rust later. We run the function when a client calls it:
```ts
// SERVER SEES THIS:
export const fn_1 = () => { ... }
export const fn_2 = () => { ... }
export const fn_3 = () => { ... }
export const fn_4 = () => { ... }

__registerJsServerFn__("scriptName#fn_1", fn_1)
__registerJsServerFn__("scriptName#fn_2", fn_2)
__registerJsServerFn__("scriptName#fn_3", fn_3)
__registerJsServerFn__("scriptName#fn_4", fn_4)
```


Here's what we make the script look like on the client. It's basically empty, except for a few exports that wrap `__callJsServerFnFromClient__`:


```ts
// CLIENT SEES THIS:
export const fn_1 = async (...args) => await __callJsServerFnFromClient__("scriptName#fn_1", ...args)
export const fn_2 = async (...args) => await __callJsServerFnFromClient__("scriptName#fn_2", ...args)
export const fn_3 = async (...args) => await __callJsServerFnFromClient__("scriptName#fn_3", ...args)
export const fn_4 = async (...args) => await __callJsServerFnFromClient__("scriptName#fn_4", ...args)
```


Behind the scenes, when the client calls `__callJsServerFnFromClient__`, it sends a reliable message over QUIC to the Server. The server then looks up the function name, calls the function, serializes the result and sends it back to the client.

To recap, we implement `'use server'` by simply transpiling the `'use server'` file differently on the server and client.

Internally we did case analysis to make sure these all behave properly:
- `export function x(){}` 
- `export async function x(){}`
- `export const x = `
- `export let x = `
- `export var x = `
- `export { x }`
- `export { x as y }`
- `export { x, x as y, y as z }`


So that's how we implement `'use server'`!

### Implementing 'use client'
We also implement `'use client'`. It's almost the same as `'use server'`, except we force the first parameter to be `user_id` because there are many users, so the server must specify which one it is calling. 

Suppose this is our Client script:
```ts
'use client'

export const fn_1 = (user_id) => { ... }
export const fn_2 = (user_id) => { ... }
export const fn_3 = (user_id) => { ... }
export const fn_4 = (user_id) => { ... }
```

Then we transpile it to this on the client:

```ts
// CLIENT SEES THIS:
export const fn_1 = (user_id) => { ... }
export const fn_2 = (user_id) => { ... }
export const fn_3 = (user_id) => { ... }
export const fn_4 = (user_id) => { ... }

__registerJsClientFn__("scriptName#fn_1", fn_1)
__registerJsClientFn__("scriptName#fn_2", fn_2)
__registerJsClientFn__("scriptName#fn_3", fn_3)
__registerJsClientFn__("scriptName#fn_4", fn_4)
```


And we transpile it to this on the server:


```ts
// SERVER SEES THIS:
export const fn_1 = async (user_id, ...args) => await __callJsClientFnFromServer__("scriptName#fn_1", user_id, ...args)
export const fn_2 = async (user_id, ...args) => await __callJsClientFnFromServer__("scriptName#fn_2", user_id, ...args)
export const fn_3 = async (user_id, ...args) => await __callJsClientFnFromServer__("scriptName#fn_3", user_id, ...args)
export const fn_4 = async (user_id, ...args) => await __callJsClientFnFromServer__("scriptName#fn_4", user_id, ...args)
```

### Example - 'use client'
Here's what our original example would look like if we used `'use client'`. This is actually the intended way to deal with UI, not `'use server'`:


```tsx
'use client'

// A built-in Kube.new helper for React updates from outside a Component.
let scores = createState() 

// The server calls this to change the client's scores.
export const setScores = (userId, newScores) => {
    scores.set(newScores)
}

// The client uses this to get the state in a Component. It's basically useState(), and updates when scores changes.
export const getScores = () => {
    return scores.get()
}

let GameUI = () => {
    const scores = getScores() 
    return (<div>
        The scores are: {scores}
    </div>);
}

render(<GameUI />)
```

The server then calls the `setScores` function:

```ts
// Server code:
import { setScores } from './the/client/script'

let scores = {
    player1: 100,
    player2: 500,
    player3: 200,
}

while (true) {
    setScores(userId, scores)
    await tick()
}
```
