# Load test prompts (sent 2026-10-01 as Kubester)

1. [game 242](https://kube.new/editor/242) — snowball fight in a little snowy village. you can pick up snow from the ground and throw snowballs, and if you get hit 3 times you turn into a snowman for 5 sec lol. build snow forts you can hide behind. use the default character. works if its just me too, add a few dumb bots that throw snowballs badly

2. [game 243](https://kube.new/editor/243) — survive the disaster game!! everyone spawns on a small island town and every 30 seconds a random disaster happens: tsunami, meteor shower, tornado, lava floor, giant bowling ball rolling through. last ones alive get a point. default character is fine

3. [game 244](https://kube.new/editor/244) — you are a bee. make a bee mesh for the player with wings that flap. fly around a giant garden collecting pollen from flowers and bring it back to the hive. other players are bees too and you can bump into each other to steal pollen. watch out for the frog

4. [game 245](https://kube.new/editor/245) — helicopter survival - a helicopter (make a real looking chopper mesh) flies over a zombie infested city and you have to get to the roof of a skyscraper before it leaves. every round the helicopter lands on a different roof. use the default character

5. [game 246](https://kube.new/editor/246) — boat wars. each player drives a little pirate boat (make a boat mesh with a cannon on the front and a sail) in an ocean with islands and you shoot cannonballs at each other. sinking boats leave a floating barrel you can pick up for health

6. [game 247](https://kube.new/editor/247) — lemonade stand tycoon but it escalates. start with a sad little lemonade stand, buy upgrades, eventually you own a lemonade factory and a lemonade rocket. each player gets their own plot next to each other so you can see how much better your friends are doing

7. [game 248](https://kube.new/editor/248) — flappy bird but it's 2d. camera is fixed from the side and you only move up and down, tap space to flap. make a chunky bird mesh and green pipes. multiplayer = everyone flaps at the same time as see-through ghost birds and you can see who dies first

8. [game 250](https://kube.new/editor/250) — battleship but in 3d. theres a room with a big table in the middle with two game boards on it. walk up to one side of the table and press E to sit down. when 2 players are sitting the game starts and you place your ships on the grid then take turns clicking squares to fire. if ur alone theres a robot sitting at the other side you can play against

9. [game 254](https://kube.new/editor/254) — a lobby game where everyone waits in a waiting room (make it a fancy hotel lobby with an elevator) and when the timer hits 0 everyone goes in the elevator and the doors open on a random floor minigame: hot lava floor, musical chairs, or a dodgeball room. default character

10. [game 255](https://kube.new/editor/255) — hot potato but the potato is a bomb and also it screams. whoever's holding it when the timer runs out explodes and gets launched into the sky. you pass it by touching someone. bots if you're solo. make a little bomb mesh with a face

11. [game 256](https://kube.new/editor/256) — grocery store dash. you have a shopping cart (make a cart mesh the player pushes) and you race around a giant supermarket grabbing items on your list. carts can crash into each other and items fly out. most items when the store closes wins

12. [game 257](https://kube.new/editor/257) — obby but the floor is made of cats. like the platforms are big sleeping cats and sometimes they wake up and stretch and you fall off. make cat meshes. default character for the player

13. [game 258](https://kube.new/editor/258) — you're a tiny mouse in a giant kitchen (make a mouse mesh for the player). steal cheese from the fridge and bring it back to the mouse hole without the cat seeing you. other players are mice too, team up or race

14. [game 259](https://kube.new/editor/259) — tower defense but everyone stands on top of their own tower and throws rocks at the waves of goblins walking by. you can upgrade your tower to be taller. if the goblins knock your tower down you fall and get sad. default character

15. [game 260](https://kube.new/editor/260) — ok so its a game where you play as a delivery drone (make a drone mesh with 4 propellers) delivering pizzas around a city. there's wind and birds that get in the way. leaderboard for most deliveries. other players can steal your pizza by flying into you

16. [game 261](https://kube.new/editor/261) — king of the hill on a giant floating donut. everyone tries to stay on top of the donut the longest, there's sprinkles that are slippery. push people off. default character

17. [game 262](https://kube.new/editor/262) — waiting lobby with a little minigolf course to mess around on while you wait, then after 60 sec everybody gets teleported to a big race: soapbox car race down a giant mountain. make soapbox car meshes for the players to ride in

18. [game 263](https://kube.new/editor/263) — farm tycoon with chickens. you start with one chicken. chickens lay eggs you sell, buy more chickens, eventually chickens are laying golden eggs and theres like 400 chickens. make chicken meshes. players have farms next to each other and can steal eggs off each other's farms at night

19. [game 264](https://kube.new/editor/264) — a pong game you play by physically standing on a giant pong paddle and walking left and right. the ball is huge. 2 players or 4 players, if alone play against an ai paddle. top down camera

20. [game 265](https://kube.new/editor/265) — ghost hide and seek in a haunted mansion. one player is the ghost (make a floaty ghost mesh, they can go through walls but only for 2 sec at a time) and everyone else hides using the default character. if you're solo the ghost is a bot. flashlights!

## What happened (round 1, 23:12–23:34 UTC)

- 20 turns ran at once on Opus 5.5. Only 248 (flappy bird) and 260 (pizza drone) finished, after 13 and 11 minutes.
- The other 18 show "Reply failed: The reply stopped unexpectedly." Each one's last model call was 14–16 min after it started. They all hit the 15-minute reply deadline in chat.ts; this was not a crash.
- Each turn used 26K–65K output tokens and 0.5M–2.6M input tokens (mostly cache reads), and 87–146 credits.
- Errors seen under load (tool results and Edgegap logs):
  - Supabase: a statement timeout (57014) on a mesh insert (244); PostgREST 503/500 on `blender_metadata` (263, 265); a failed request to `blender_metadata` (246).
  - Starting the Blender subagent failed: a 500 (245) and "error sending request" (258, 262).
  - Supabase storage returned 429 Too Many Requests on a `mesh_data` fetch (242's log).
  - The editor's auth.ts timed out (DOMException TimeoutError, 242's log). On reopening, 242 first showed "Error: The editor request failed."
- 264 (pong) called report_issue: no API to set a top-down camera.
- The Edgegap container-logs API answered 429 after 1 request, so I only have logs for 242 and 265.

## Round 2

At 23:42–23:50 UTC, the 18 stalled games each got a casual "it stopped, keep going" message.

## Round 2 results (23:39–23:58 UTC)

- **Finished:** 242, 244, 246, 247, 250, 255, 261, 263. With 248 and 260 from round 1, 10 of 20 games are now done.
- **Stopped at 15 minutes again:** 243, 245, 256, 259, 262, 264, 265.
- **Whole reply failed with "canceling statement due to statement timeout":** 257, 258. The database timeout now ends replies, not just single tools.
- **254 has no reply at all:** its only model call ended at the 15-minute mark, and the empty reply was deleted. The page shows Retry on the user's message.
- Tool-level errors this round:
  - 245: the helicopter mesh failed again on the modeling service.
  - 259: a batch of asset downloads timed out.
  - 244 and 265: one mesh each failed with a server error.
- Kubester has 7,512 credits left.
