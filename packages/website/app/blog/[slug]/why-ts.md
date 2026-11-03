---
title: Why TypeScript?
description: Why we made our scripting language TypeScript.
isPublished: true
date: 2026-08-17
priority: 1
author_id: 17
author_name: Andrew Pareles
thumbnail_image_url: /blog/why-ts/thumbnail.svg
---




We built Kube.new's game engine from scratch to support a new age of games. At a high level, this involved two big engineering decisions:

(1) We allow users to build games in the browser, yet still play games natively on desktop. While significantly more challenging than simply building browser games, we think this is a requirement when building a platform to last.

(2) We support TypeScript!

This blog post is focused on the benefits that convinced us that TypeScript was the right scripting language for Kube.new.

![A picture of the editor with a TypeScript script open](/blog/why-ts/Script-screenshot.png)

### 1. UI

TypeScript's React is the main way people build UI today. By using it natively on our platform, AI automatically understands how to build robust, responsive, and polished game UI.

For example, the following React GUI code is fully supported on Kube.new:
```tsx
// We implement all of this!
const MyComponent = () => {
    const [state, setState] = useState(5)
    return (<div className="top-center">
        The state is: {state}
    </div>);
}

render(<MyComponent />)

```


### 2. Server/Client Communication

React established the convention of using `'use server'` to communicate between the server and client. We built this protocol natively into Kube.new. We also built our own opposite-direction variant `'use client'`.

These protocols are the primary channel that our Server and Client use to communicate. For example, a server script on Kube.new is allowed to look like this:

```ts
'use server'

export function clientCanCallThis(...args) {
    // Call this from the client!
}
```

A client script on Kube.new is allowed to look like this:

```ts
'use client'

export function serverCanCallThis(userid: number, ...args) {
    // Call this from the server!
}
```

We discuss how we implemented these networking protocols over QUIC in our deep dive, [Script Networking](/blog/use-server). 

### 3. Built-In Type System

One of the biggest benefits of TypeScript is the strong ecosystem around type-checking. It is very lightweight to lint all the scripts in a game and expose diagnostics every time the code changes. It is also portable, and fits into our in-browser editor, allowing for code completions. 

It also enables us to compile all our types into a [TypeScript API](/docs) docs page.


### 4. Built-In Modular Import

TypeScript supports imports and exports. This is a major benefit over a language like Lua. Kube.new natively supports modular code this way. 

```ts
// This is valid in Kube.new!
import { functionName } from './child_name/script_name'
```



### 5. Lightweight

TypeScript is lightweight enough to run on both the server and the client.

We take advantage of client-side code execution frequently. For example, any creator can write custom character-controller logic purely in TypeScript. This means games can have custom character physics, from walking to controlling vehicles and flying planes. 


```ts
// This is valid in Kube.new!
setTimeout(() => {
    const brick = getObjectByName("my-brick")
    brick.location.x = 20;
}, 1000)
```




### 6. It's Not That Hard For Us To Implement


Kube.new is built using Rust. In order to run TypeScript, we use Deno which runs on V8.

Deno allows us to increment one step of awaiting microtasks by calling `poll_event_loop`[^eventloop]. This is essentially a "tick" of scripts, which is exactly the abstraction we would want from an async library. Before the physics tick, we simply run a script "tick".

[^eventloop]: It starts all the currently `await`ing code, runs it, and then yields when the next layer of `await`s is reached.

Our core systems run on Rust, but Deno makes it relatively easy to interface into it through TypeScript. We built our own Node package that we expose to every script through "Deno ops", which are Rust functions callable from TypeScript. To build a convenient API, we simply had to wrap the [`LiveObject`](/docs#LiveObject) in a JS Proxy and call our Rust backend.

Below, we show some of the convenient code you can write using TypeScript.

 

```ts
let brick = getObjectByName("my-brick")

// These all actually affect the brick!
brick.location.x = 10
brick.location = { x: 20, y: 20, z: 20 }
brick.movement.x = 30
brick.movement = { x: 20, y: 20, z: 20 }

brick.color = { r: 10, g: 20, b: 30 }
brick.isPushable = true
brick.isWalkthrough = true

// This continually reads the current value of x!
while (true) {
    await tick()
    console.log(brick.location.x)
}
// ...
```


We also implement a watchdog thread to terminate scripts that stall. The watchdog is based on CPU-time, which is robust unlike wall-clock time which can incorrectly terminate scripts during high load or spikes. 

In order to yield until the next tick, a script can await [`tick()`](/docs#tick):

```ts
while (true) {
    // Do something...
    await tick();
}
```


When writing scripts, the creator needs to make sure they yield sufficiently often to prevent blocking, or the scripts will be terminated. TypeScript does not modularize execution, meaning if one script stalls then the watchdog must stop all scripts. This is the one downside to using TypeScript; however, the watchdog is based on CPU-time, so this will never happen unless the code is genuinely written in a way that stalls.


## TypeScript

We're very excited to be using TypeScript as the scripting language for Kube.new. There are many benefits and we're happy to be innovating here.
