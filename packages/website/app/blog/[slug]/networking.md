---
title: Networking Deep Dive
description: How we built our networking from scratch. Timestamps, QUIC protocols, etc.
isPublished: true
date: 2026-10-02
priority: 5
author_id: 17
author_name: Andrew Pareles
thumbnail_image_url: /blog/networking/thumbnail.svg
---


I built Kube.new from scratch in Rust, together with GPT 5.2-6 and Opus 5.5.

This was a big endeavor. This post is specifically about designing the networking for game servers. I also built custom shaders, object types, web editor, desktop app, and hosting architecture, but that's for a different blog post.


There is a fundamental problem you face building any game engine: it takes time for your inputs to reach other players and vice versa. When building the networking for Kube.new there were two axioms I established we must follow:

**Axiom 1:** When a person presses WASD, they must move immediately on their own screen, even though the server won't have received the message yet. This axiom is easy; it means we must always immediately apply inputs to the player's own character object.

![player responds immediately to WASD](/blog/networking/you.svg)


**Axiom 2:** Players should see the world's changes happen in sync, even though the changes might arrive delayed by varying amounts of time. In order to make changes that are synchronous actually appear synchronous, we make each player's world be delayed in time. This allows them to buffer all the changes and apply them in sync when they're due.
![non-player objects are delayed](/blog/networking/delay.svg)

This axiom is harder, because we need to (1) pick a world delay amount, which we'll call T and (2) implement timestamps.

We'll talk about timestamps below. For now let's just talk about picking T. It's trivial that T must be at least one's round-trip ping[^2], or else there's no point in having the delay in the first place; anything that reached the player would be after the delay, so they'd miss the effect completely. So at minimum, let T = ping. We add on an extra 50ms for jitter (packets take a little extra time to arrive), 16ms for extra time to interpolate between the latest 2 physics steps (16ms = 1/60Hz = physics step period), and a little extra time to give players with higher ping more time to reach other clients (~30ms).

T = ping + 50ms + 16ms + 30ms.


To recap, on each client, all objects live T milliseconds in the past, except for: (1) their own character and (2) anything affecting it like WASD inputs, animations, etc.[^3]

[^2]: It's tempting to make T equal to the laggiest player's ping, so that a client can smooth even the laggiest player. However, that allows high ping players to make the game a more delayed experience for everyone, so we decide to just not do that. Instead, we just make laggy players appear on their own high-latency delay, not synced with the rest of the objects.

[^3]: For completeness but unrelated to our discussion, also (3) client-side timers, which only run locally so have no reason to be delayed.

## Server-Side

On Kube.new the server is authoritative, meaning it is the only place that can create, move, and delete objects that everyone sees. This means it needs to communicate the world's changes with each player, ~60 times a second. This sounds simple, but it requires implementing a networking protocol. 

We are using QUIC datagrams for our networking so we don't deal with the problems of TCP[^tcp], but that comes with two problems we then have to address: packets can be dropped, and the byte limit is 1200 bytes. 

[^tcp]: TCP works on the OS level by waiting for packets to arrive in perfect order. This means if one packet is dropped, the entire game freezes and waits for it to arrive.

To deal with the byte limit, we ensure all objects in our world take up O(1) space. For example, it's sometimes tempting to have a struct like this which can grow arbitrarily large:
```rs
// ⚠️ Objects should never have arbitrary size. ⚠️
struct BadAnimationObject {
    duration: f32,
    frames: Vec<Keyframe>
}
```
However, we instead always break objects down into O(1) components like this:
```rs
// ✅ All objects are O(1) size.
struct GoodAnimationObject {
    duration: f32,
}
struct Keyframe {
    parent_animation_id: u32
}
```
This is a small design detail, but it means we never have to break down big objects into multiple packets since all items are below 1200B. 

This simple design decision allows all objects to be treated the same in the netcode. We just have one algorithm to build a message to the client: keep cramming objects into the message to the client until it's full, and then send the message. 


The server sends `ClientMsg` to each client, 60 times a second.


```rs
// Changes for 1 object.
struct ObjectDelta {
    parent_id: Option<ObjectId>,
    physics:   Option<Vec12>,
    color:     Option<Color4>,
    name:      Option<String>,
    etc:       Option<EtcEtc>,
}

// We want to send this to each client 60x/sec.
struct ClientMsg {
    server_timestamp: f32,
    message_num: u64,
    changes: HashMap<ObjectId, ObjectDelta>,
}
```






We've deduced that `ClientMsg` is the shape of message we must send to each client. Now we simply need to work out the details of constructing it. Namely, if the server just made lots of changes, how does it remember them all? It might not be able to fit all the changes it wants to send at once.

We can reason that the server must remember all the "dirty" changes that eventually need to get sent. Thus every tick, the server adds all the items that changed to a "dirty" queue. It then pops as many items as it can cram into the next message. Those items might get dropped over the network and need to be resent, so we can't just delete them the moment we send them. That means we need another queue called "unacked" that we move the items into. We resend unacked items if they haven't been ACK'd for a while.


To recap: the server stores 2 queues. A dirty queue to remember all the changes it needs to send, and an "unacked" queue so it can resend changes that were never ACK'd by the client.
![The dirty and unacked queues communicating with the client](/blog/networking/resend.svg)


Here's the idea in code[^addremove]:

[^addremove]: In reality our message to the client also includes `Adds`, `Removes`, and `ForeignModifies` (other players' inputs) - not just `ObjectDelta` - but they're all versions of the same idea.


```rs
// Changes for 1 object. Just indicator bools saying what to read + send.
struct ObjectDeltaFlags {
    parent_id: bool,
    physics:   bool,
    color:     bool,
    name:      bool,
    etc:       bool,
}

type DirtyQueue   = HashMapQueue<ObjectId, ObjectDeltaFlags>
type UnackedQueue = HashMapQueue<ObjectId, ObjectDeltaFlags>

// 60x/sec the server calls this for each client:
fn send_client_msg(client_id: u64) {
    let mut dirty   = dirty_queue[client_id];
    let mut unacked = unacked_queue[client_id];

    let client_msg: ClientMsg = f(&mut dirty, &mut unacked);
   
    sendTo(client_id, client_msg); 
}
```

The queues are organized by `ObjectId`, not `message_num`, so that we can be very granular and avoid sending individual objects that were already sent more recently.








## What The Client Does

Now we know the shape of message that the server sends to each client, `ClientMsg`. It contains all the changes the client should apply, like "add a ball to XYZ position" or "move this player to XYZ". The client uses the timestamps on the messages to apply changes at the correct time.

There are a few problems, though - a client can receive their `ClientMsg` packets reordered, duplicated, or dropped.

![packets arriving reordered, duplicated, and dropped](/blog/networking/inconsistency.svg)

We can fix this by having the client simply accept the latest `message_num` that it gets given the same object id. That way, **eventually** the client will get the correct updates. However, that only works if all changes commute (are order-independent)! This is a more powerful statement than idempotence, so it's a little hard to get right. 

For example, if an object's parent wasn't received yet but you wrote all your logic to assume that (like I did), then you will have ordering bugs. It's basically hysteresis for your game due to errors in one order but not the other. The packets could genuinely arrive in any order, and you need to allow for both orders if you want commutative behavior.



```rs
fn createObject(object_id: u64, parent_id: u64) {

    let Some(parent) = getObject(parent_id) else {
        // ⚠️ Not allowed if we want pure commutative behavior! ⚠️
        return Err("Parent does not exist")
    };

    // ... More logic to set up the object ...
}
```

I found myself (and Opus) writing very complex logic to get commutation to work between creation of objects and their parents, and it wasn't scalable. So I removed the commutative requirement on this function, and simply made the client just wait until the parent *does* exist, on the network layer. This way the client never sees an object change until it's already received the parent.

This feels a lot like re-implementing TCP - we're waiting for packets to arrive in a certain order on the networking layer! However, it's not a perfect linear ordering like what TCP implements; it's a local topological sort that simply waits for parents before children. It's very localized. In practice, modifying parents is rare anyway, and usually the desired behavior is to wait[^ifyoudid].

[^ifyoudid]: If we allowed the children of a RigidGroup to be created before the parent arrived, the client would see a sudden snap when the parent RigidGroup did arrive. In this case and most others regarding parent/child, commutation is undesirable and we just want to wait.


```rs
// Client runs this as their network layer:

fn popDueChanges() -> Vec<ObjectDelta> {
    let latest_msg: ClientMsg = network.recv().await;

    let mut changes_due = Vec::new();
    for change in latest_msg.changes {
        // ensure T ms in the past
        if !change_is_due(latest_msg.server_timestamp) {
            // store for later
            continue;
        }

        // ensure objects have a parent
        if !has_parent_yet(change) {
            // store for later
            continue;
        }
        
        changes_due.push(change);
    }

    return changes_due;
}
```

We had similar complexity problems trying to write code for adding, modifying, and removing objects. They simply don't commute. For example, it's not possible to modify an object without knowing what shape it is, which you only learn in the "add" message. So again, the fix is to guarantee ordering. We need to guarantee the client gets messages in the order `Add -> Modify -> Remove`.




We implemented this slightly differently from the parent/child idea. The ordering guarantee is done by the server, not the client. The reason is that it's best practice to make sure the client got every modify before removing the object, and that's much simpler for the server to do than the client. While the server is doing that, it might as well also guarantee the `Add -> Modify -> Remove` ordering. Again, this is like re-implementing TCP, but it's a *very* local sort, not a full linear TCP ordering.

```rs
// This lives on the server.

fn send_client_msg() {
    // ✅ Slightly modify this function we wrote earlier.
    // We guarantee the client:
    // 1. ACK'd Add before sending Modify, 
    // 2. ACK'd all Modifies before sending Remove
}
```

With these changes, all the functions commute, and the networking is valid!


At this point we have robust server-to-client communication.

Now we need to figure out the other direction, client-to-server communication. Here's what we want the client to send:
 

```rs
// Clients send this to the server 60x/sec.
struct ServerMsg {
    input_num: MessageNum,           // client's message num
    estimated_server_timestamp: f32, // estimated time on server
    acks: Vec<MessageNum>,           // msgs that are being ACK'd
    location: PhysicsInfo3D,         // where the character is
}
```
This is very close to what we end up implementing. However, there's a small problem if we just implement this. Notice that both the client and server can move the character. If the client just sends the `ServerMsg` 60 times a second as-is, then the server won't be able to make changes to any clients (e.g. teleport them). The change will happen for a split second, and then the client will just override it.

```ts
// The server runs this:

let players = objectService.getObjectByType("player")
let somePlayer = players[0]
let character = objectService.getObjectById(somePlayer.characterObjectId)

// ⚠️ The client will incorrectly override the server in these cases! ⚠️
character.location.x = 10
character.movement.x = 20
character.location = { x: 10, y: 10, z: 10 }
character.movement = { x: 20, y: 20, z: 20 }
somePlayer.characterObjectId = 56
```


We can fix this by making the server ignore all of the client's messages that are about their own character, until the client has ACK'd these specific-case changes. 

```rs
// ✅ Server runs this to fix the above problem.
fn receive_client_msg(client_id: u64, msg: ServerMsg) { 
    let mut acks_needed: &mut HashSet<MessageNum> = acks_needed_of_client[client_id];
    for ack in msg.acks {
        acks_needed.remove(ack);
    }

    // Only apply the client's changes to the character if acks_needed is empty.
    if acks_needed.is_empty() {
        // Update character.
    }

}
```





This completes the networking protocol between server and client!

## Clock Synchronization

It's important to have accurate timestamps if we want clients to properly show a delayed world. But how do we actually sync clocks across the network to get accurate timestamps?

Well, we use NTP (Network Time Protocol). The basic idea is that the client constantly pings the server asking it for its current time. The server responds, and the client corrects for how long the response took and averages out those responses. It's one of the simplest possible algorithms you could devise to do timestamps, and it's conventional[^syncing_options].

[^syncing_options]: A simpler algorithm might be to just use `estimated_server_time = initial_time_server_said + current_rtt()/2`. However it could theoretically drift if anyone's clock is relatively slow. 

This means the time will practically be the same time no matter where you read it!

We even use timestamps in other places - you can do complex animations with [`PropertyAnimator`](/docs#propertyAnimator) or [`SkAnimator`](/docs#skAnimator). They both take advantage of the timestamps to sync animations, using [`Clock`](/docs#clock) objects.











## Physics Engine



The physics engine we're using has two types of objects: dynamic (experience physics) and kinematic (do not experience physics). We decided the objects that you own like your character should be dynamic so you can simulate them, and everything else should be kinematic. For example, if the server tells you an object is at x=100, you just move it there kinematically, and it never moves away.


### Client
- Dynamic: Your character.
- Kinematic: Everything else.

### Server
- Kinematic: All characters.
- Dynamic: Everything else.

This makes control very simple and guaranteed to be correct. However, one negative consequence is that you can't really push things unless scripts are involved, because basically everything is kinematic on your client. We may add a fix for this which requires transferring object ownership to the client, but we're focused on great multiplayer right now, not physics-based games.



## Thanks For Reading!

This wraps up our summary on the developer decisions that went into creating the networking between server and client. We put a lot of human effort into our platform to create robust communication protocols, so that neither you nor your AI need to think about the complex details. 


There are a lot of interesting details about how we allow you to edit & test games, live, with your friends, and live with any AI via MCP. Another blog post. 






