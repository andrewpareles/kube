---
title: Animations
description: Building our custom animation system.
isPublished: true
date: 2026-08-24
priority: 5
author_id: 17
author_name: Andrew Pareles
thumbnail_image_url: /blog/animations/thumbnail.svg
---



When building Kube.new, it was completely up to us to decide how you should be able to animate objects. 
This blog is about how we designed our animation system.


## Skeleton Animators

In most places online, people animate characters using Skeletons. Skeletons are essentially just collections of points you move around. Your character follows the points in the skeleton.

So we created a Skeleton animator - an object that simply moves around a Skeleton. It points to three objects: 1. the Skeleton it should animate, 2. the animation to play, and 3. a Clock object that lets you start and stop it. 


Here's what it looks like: (And here it is in our API: [`SkAnimator`](/docs#skAnimator))

![how skeleton animations are organized](/blog/animations/skanimator.svg)


All we need next is a way to attach an object to that Skeleton to move along with it. We did that by adding the [`SkAttachment`](/docs#skAttachment) type which lets you "glue" items onto the Skeleton.
![skattachments - how to attach objects to a skeleton](/blog/animations/skattachment.svg)

We were happy to find the AI responds well to this format! AI has seen millions of Blender files online. Our implementation is based directly on the GLTF format, which is the standard for storing skinned meshes. This format is clearly similar enough to our implementation to have some transfer.






## Property Animators

We also wanted to support general animation, not just animation based on a Skeleton.

To do this, we introduced [`PropertyAnimator`](/docs#propertyAnimator). It is very similar to the Skeleton Animator above, except it takes in the name of any field you want to animate, instead of pointing to a Skeleton. 

![how property animations are organized](/blog/animations/propertyanimator.svg)


You can animate any field, as long as it's a continuous number (position, velocity, color, texture width, anything!).


## Tweens

The above animators work well when you want to bake an animation, but they are a little heavy when you want to dynamically move objects. For example, if you wanted to dynamically move a brick from point A to B, where A and B are not predetermined, then you would need to modify a handful of Keyframe and Clock objects in order to do that.

We expose a cheaper alternative: Tweens. 


Tweens are basically PropertyAnimators with just two keyframes, but they are cheaper over the network, and they delete themselves when they're done. We expose [tweenTo()](/docs#AnimationService), which creates a `Tween`. Just like PropertyAnimators, you can use Tweens to animate position, velocity, color, etc.




## Blending

In order to deal with multiple animations on the same object (like Walking, Jumping, Idle on one character), we let each animator specify a `priority` and `weight` field. The animations are blended naturally in order of priority, until the total weight reaches 100.

For example, if you want to smoothly transition your character from Walking to Jumping, all you need to do is have two `SkAnimator` objects pointing to it, and transition the Walking weight down to 0 and the Jumping weight up to 100. We expose the [smoothlyChangeWeight()](/docs#AnimationService) function in TS so that this is easier. It literally updates every frame (not just per physics tick), so transitions happen very smoothly (240fps+).



## Clocks
The [Clock](/docs#clock) object is used internally by both animators. It is a special timing type that lets the server and client agree exactly when the animation started/stopped. It uses timestamps under the hood, which we discussed in our [Networking](/blog/networking) blog.


```rs
// Simplification:
struct Clock {
    started_at: f32,                // a timestamp
    clock_location: ObjectLocation, // either Server or Client
    should_loop: bool, 
    speed: f32,       
}
```
## High Framerate

One of the most interesting parts about our Desktop app is that we separate out our Physics thread from our Renderer thread. This allows us to simulate physics at the same rate on all devices (60 times/sec) while rendering the game at 240fps+!


Interpolation is the single mechanic that gives rise to higher framerates. To get a high framerate, we simply have the renderer thread interpolate between the latest two snapshots that the physics thread has given it.

![snapshot interpolation](/blog/desktop-app/renderer.svg)



We interpolate each object's position and velocity between the two most recent snapshots (using Hermite polynomials). The interpolation method can be customized when animations are involved (for example, stop-motion). Internally, we recompute skeletons per rendered frame too, so that skeleton animations are high-framerate.

Interpolation is built into our custom shader logic, so it also works when animating shadows, texture sizes, etc.









<!-- 

## Physics Engine

Once we decided the objects we wanted to allow above, it was time to actually implement them in the physics engine.



First, we wanted to prevent Animators from physically affecting the same object. Blending happens automatically given the *same* kind of Animator, so we only had to worry about the different `SkAnimator` and `PropertyAnimator` colliding. It was pretty natural to just say that `PropertyAnimator` can only affect non-subitems physically, since `SkAnimator` only affects RigidGroup subitems physically. 

After we did that, we were ready to implement the animators in the physics engine. Here are the 3 possible ways that objects can move in Kube.new.

### 1. - SkAnimator

When animating a RigidGroup attached to a Skeleton, we simply need to move subitems relative to the center of the RigidGroup. Rapier exposes methods for this. This is simple.

### 2. - PropertyAnimator
If you have a PropertyAnimator moving a platform position-by-position, you want its velocity to be set based on the position. If it moves the platform 100 blocks, the velocity should reflect that and be high. 
`setPhysicsInfo3D(position?, velocity?)`
- position - if kinematic, move it kinematically using position, replacing velocity. if dynamic, give an error - too confusing[^propertyanimatorthrow].
- velocity - update velocity.

[^propertyanimatorthrow]: This function is just used for PropertyAnimators. It's annoying to implement the dynamic object case when changing position, and PropertyAnimators are just confusing if they move dynamic objects' position anyway. So we specifically just give an error in this instance.


### 3. - Scripts

If you use a Script to set an object's position to x=100, you don't want velocity to be set based on position. It should have independent velocity.
`forcePhysicsInfo3D(position?, velocity?)`
- position - teleport it to the new position, but keep old velocity.
- velocity - update velocity.


Also, if you set an object's position using a Script, you need to reset the interpolation that the renderer thread does. We account for this.




## Other






 -->















<!-- 
---
title: Desktop App
description: Building the Kube.new desktop app.
isPublished: true
date: 2026-08-19
priority: 5
author_id: 17
author_name: Andrew Pareles
thumbnail_image_url: /blog/desktop-app/thumbnail.svg
---



This post is about some of the thinking that went on behind the scenes to build our desktop app. 


## Desktop App Modes
There are two modes the app can run in: Edit or Play. To launch the app in one of these two modes, the website simply redirects you to the deeplink `kube-new://` with some parameters. 

Here is a dependence tree of the desktop app and the different modes.


![Client modes](/blog/desktop-app/Client-Modes.svg)


- **Play Mode:** you run Play Runner and connect to a server running Server Runner.

- **Edit Mode:** you run your own local Play Runner and Server Runner when you playtest.[^headless] 

[^headless]:  The AI can also do this in headless mode.




 -->



