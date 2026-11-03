
# Networking notes

Andrew wrote this note to think through networking in early development.  You can just read the TLDR for the important parts.

### TLDR 
- The SERVERs always enforces that the client receives a valid order of add->modify->remove for each individual object_id, so the client doesn't remove before it adds etc.
- But the server does not enforce any order between DIFFERENT object_ids (like children or pointers), that would be very complicated.
- The CLIENT allows for this by making all add/remove/modify operations commute on children and pointers (even when invalid temporarily), and going with last-message-wins.

Misc details
- If an object is removed while sending an add/modify, a dummy add/modify is sent so the client can ACK it and the unack'd data structure will continue making progress.
- The client properly handles out of order messages by having modifys commute, adds/removes idempotent, and the client tracking recent removes so a crazy delayed unack'd add doesnt happen after it's already been removed.
- snapshot buffer is responsible for applying all of the deltas, so that they only appear in the 100ms after they were received

- we do topological ordering of adds/change parents on the client - it's not perfect linear ordering so is still parallel and efficent in many cases, but it also *guarantees* that a parent is added before any child (and has no cycles). the server throws an error on these things, so an invalid state will never be awaited forever by the client. we guarantee a parent is added before its children!

- "snapshot" means UDP message from client to server. message_num is for the snapshot. we wait for ready-to-start to start sending UDP messages, but not TCP since TCP is guaranteed to be received by us from the stream, while UDP will simply be dropped.


- TODO: on the server, we need to validate the client function outputs, and server function inputs. Ideally using Typia.

### Server side
Things we must enforce on the SERVER SIDE per object_id object to make sure client always gets valid operations:

For server (when adding to dirty):
Adds/removes do not make sense to mark as dirty with any other operation.
- If there is a 'remove', delete any add/modify, from both existing dirty and also what we're about to add
- If there is an 'add', delete any modify (remove is already handled through 1st step)

When choosing what to send:
- Dirty will always be perfectly ordered/have no overlaps because of the above
- But as-is we might still send an add, then a delete in the next message, and the client gets delete then add, this is bad
- To fix, we could make the client remember past removes if they came before an add, and avoid errors when modifying nonexistent objects
    - This is overcomplicated
- The fix we use: server option: always enforce Add -> Modify -> Remove for the client, by awaiting their ACKS in that order
    - If 'add' is in unacked, dont send user any new modifies/removes of that object until the add is ACK'd
    - If 'modify' in unacked, dont send user any new removes of that object until the modify is ACK'd
    - TODO: Issue: adversarial non-ACKing memory overflow...?

### Client side
Things we must enforce on the CLIENT SIDE to make sure client always gets valid operations:

Operations on the same object
- We enforce this is perfectly ordered on the server.

Operations on different objects
- Children
- Pointers
- We must either enforce order of operations somehow (annoying for scripts anyway),
    OR (our solution:) make them commute and allow non-existent/invalid children and pointers temporarily, last change wins.
- Our solution is valid because:
    - We will never have A.x = C and B.x = C in the same message, server order is always valid
    - Server always sends last-message-wins operations. We just need to let them commute.

- Children:
- We must allow non-existent parent/children temporarily.
    - The only time children are modified is when we add/remove/change parent.
    - Easy to make add/remove commute
    - RigidGroup
        - add => refresh children
        - remove => fine since recursively remove all children anyway
        - change parent => TODO: i think same as refresh children when add, but we havent built change parent yet
        - we can always get the object, it just won't have some children but that's fine

- Pointers:
- We must allow non-existent/invalid pointed object ids temporarily.
    - Adds: add object which is pointed to or from
    - (NEW)->() => this is fine
    - ()->(NEW) => we must allow invalid pointer temporarily

    - Removes: delete object which is pointed to or from
    - (X)->() => we must allow invalid pointer temporarily
    - ()->(X) => this is fine

    - Modifies: `A.x=invalid`
    - ()->(invalid_id) => we must allow invalid pointer temporarily
    - (invalid_id)->() => unreachable state

    - We can let invalid pointers with last-message-wins by simply revoking / changing the pointer to
        the latest item it saw.
    - Eventually the correct item will be pointed to since same object A is properly ordered (A.pointer = ...)



## ACKs (this part is ai written)

<details>
  
- The client ACKs each snapshot with `ClientMessageUDP::Ack` from its datagram task (`start_receiving_server_messages_udp_in_bg` in `play_runner.rs`) the moment it arrives, before the play loop takes it in.
- The server sends a change again once its message has gone unACKed for `JITTER_MARGIN_MS` plus a round trip, and sending it again gives the change a new message number, so an ACK for the old number no longer counts. ACKs used to ride on inputs, which go out once per play loop tick. A client whose ticks took longer than that margin never ACKed in time, so the server sent the same changes forever, and handling them made the client slower still.
- A pose made before the play loop applied a snapshot cannot include that snapshot's changes to the player's own objects. So each input carries `latest_applied_message_num`, the newest snapshot the play loop had taken in. The server keeps `poses_wait_for_message_num`, the newest snapshot that added the player or their character, moved the character, or changed the player's info, and `shouldIgnoreForeignCharacterDTO` ignores poses from inputs that had not applied it yet, along with poses whose change is still dirty or unACKed.
</details>

## GameChat (this part is ai written)
<details>
  
- Player chat uses a subscription stream for incoming messages and a separate request stream for each submission on the authenticated QUIC connection. The handshake stream carries only hello, initial world, and `ReadyForUDPMessages`, then finishes. GameChat stays separate from the handshake, UDP world snapshots, and script function streams.
- Client setup starts chat with `start_receiving_game_chat_messages_tcp_in_bg` in `game_chat_networking.rs`. Opening the stream, sending, and receiving all happen in its background worker. Startup does not wait for chat. The worker owns only chat and never closes the game connection or changes the application's screen; the HUD shows when its subscription closes. A failed submission is logged and does not stop later submissions or incoming messages.
- After authentication, reliable client messages use `ClientMessageTCP`, with content carried directly in `GameChat(ClientGameChatMessage)` and `ServerFnCall(ServerFnCallMessage)`. The chat stream starts with `SubscribeToGameChat` so players receive history and live messages before they type anything. Each chat submission and script call uses `sendTcpRequest` to open a stream, send its message, finish the request, and read one response. `start_receiving_client_messages_tcp_in_bg` in `server_networking.rs` starts the background listener, reads each request, and dispatches it independently alongside gameplay datagrams. One connection can have one active chat subscription. A failed or stalled stream affects only that stream.
- For each submission, the client sends `ClientMessageTCP::GameChat(ClientGameChatMessage { message })` and receives `ServerGameChatResultMessage` with success or a validation error; the server assigns `userid` and `time_live_server_ms` (Unix milliseconds). On the subscription stream it sends `ServerGameChatMessage::History` once, then ordered `Message` events, including the sender's own messages. Submissions also work before subscribing and are included in later history. GameChat protocol types live in the GameChat section of `protocol/messages.rs`.
- GameChat messages also carry the username already verified during connection authentication. The panel uses the UI theme's blue for usernames and its contrasting text color for messages, without displaying the timestamp. Keeping the username with each message preserves it for players who join after the sender leaves, with no separate username lookup or database dependency.
- Each running server owns its chat history in memory through `ServerGameChat`, grouped with the subscription sender in the GameChat section of `server_networking.rs`. Joining players get the most recent 100 messages, and history disappears when that server stops. History and live subscriptions are captured under the same lock, so joins cannot miss or duplicate a message. Clients also keep at most 100 messages. Submissions are limited to 500 Unicode characters and processed at most twice per second per connection, across all request streams; a slow receiver cannot block other players. The client waits for each submission response before sending the next so its messages stay ordered.
- The desktop player shows chat in the upper right. The play webview draws it (`GameChat.tsx` in `packages/play-webview`), and `GameChat` in `game_chat.rs` owns the chat's end of the connection and sends the page what to show. `/` focuses the input without inserting the slash; Enter sends and returns to the game, and Escape leaves the input. While chat is focused, typing goes to the chat instead of the game. Scrolling or left-clicking the chat panel does not zoom the camera or trigger game clicks, but a right-drag there still turns the camera.
- Every message goes through the `rustrict` profanity filter in `ServerGameChat::submit`, before it reaches history or any subscriber, so each way a message can be delivered shows the same censored text. The filter reads through spacing and punctuation, so `f.u.c.k` is caught along with the word itself, and each matched character becomes one hash, which keeps the 500-character limit above true. Chat censors rather than rejects, because a player who typed one bad word should still be heard; a username has to be rejected instead, since nobody can be called `####`.
- A server script listens for messages with `game.gameChatService.addGameChatListener`. `ServerGameChat::submit` queues a `JsCallbackInputs::GameChatMessageSent` on the same channel physics and player join use, so each message is delivered on its own and two that arrive in one tick both reach the listener rather than the later one replacing the earlier. A listener reads the censored text and the verified username, so a script shows what the players already saw. Client scripts have no chat listener; the server is where a message is accepted.
- Official servers log chat to the `game_chats` table. `log_game_chat_messages_forever` in `server_networking.rs` reads the room like any other subscriber, so no database request sits in the chat delivery path and a failed write only loses the log, never the message. Whatever arrived while a write was in flight is written together in the next one, and falling far enough behind to lose messages is logged as an error rather than passing quietly. Each row carries the server's Edgegap request id, the time the server accepted the message, the sender's user id, and the censored text. Playtest servers do not log, because they run on somebody's machine with no service credentials and no server id.
</details>