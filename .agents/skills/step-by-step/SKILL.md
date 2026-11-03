---
name: step-by-step
description: Use this skill if the user asks you to reapply the change step-by-step so they can follow all the changes.
---

If the user asks you to do this, they want you to take the existing changes in the worktree, stash them, and then reapply them top-down step by step so the user can digest the changes that were made more easily. 

Top-down means roughly to take steps top-down in the call tree, one feature at a time. For example, implement the call sites, new structs, and high-level architecture changes *first*, and then afterwards implement the changes that actually define the new functions that those initial places call and use. That way, the user gets to approve the high-level changes first without having to understand all the low level implementation details.

Typically this involves -
1. Come up with a step-by-step plan to apply the current changes in a top-down way. 
2. Tell the user and stash the current changes.
3. For each step, apply it and then wait for the user to tell you they approve it or want to make changes.


## A few guidelines:
- If the user asks you a question, it does not mean you should implement a change. They might just be asking you a question.
- If the user tells you to change something in one of the steps, don't start making changes or applying new steps. Think about what the consequences might be and tell the user, but don't go applying them until the user asks.
- The user might make changes to the code as you're paused. It is your turn to review the changes the user makes and tell them if anything is wrong before moving on.
